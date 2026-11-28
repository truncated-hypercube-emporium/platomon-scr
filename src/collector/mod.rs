//! Background collector thread. Polls the system at `poll_hz` and publishes
//! the latest immutable [`RawMetrics`] snapshot.

pub mod gpu;
pub mod sensors;
pub mod system;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Throughput of one disk or network adapter, bytes/s.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IoSample {
    /// Adapter name, or disk name.
    pub name: String,
    /// Alternative name accepted for selection (a disk's mount point).
    pub alt: Option<String>,
    /// read / rx
    pub a_bps: f64,
    /// write / tx
    pub b_bps: f64,
}

impl IoSample {
    /// Case-insensitive match on name or alt, ignoring trailing path separators.
    pub fn matches(&self, want: &str) -> bool {
        let norm = |s: &str| s.trim().trim_end_matches(['\\', '/']).to_ascii_lowercase();
        let w = norm(want);
        !w.is_empty() && (norm(&self.name) == w || self.alt.as_deref().is_some_and(|a| norm(a) == w))
    }
}

/// One raw sample. `None` means "this platform / machine can't provide it".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawMetrics {
    /// Increments with every published sample.
    pub seq: u64,
    /// Seconds since the previous sample.
    pub elapsed: f64,
    /// System-wide CPU utilisation, 0..=100.
    pub cpu_total: Option<f32>,
    /// This process's CPU utilisation on the same system-wide 0..=100 basis.
    pub cpu_self: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    pub disk_read_bps: Option<f64>,
    pub disk_write_bps: Option<f64>,
    pub net_rx_bps: Option<f64>,
    pub net_tx_bps: Option<f64>,
    /// 0..=100
    pub gpu_util: Option<f32>,
    /// 0..=100
    pub gpu_mem_util: Option<f32>,
    /// Each logical core's utilisation, 0..=100 (empty when unavailable).
    pub cpu_cores: Vec<f32>,
    /// Temperatures, clocks and voltages.
    pub sensors: sensors::SensorSample,
    /// Per disk (empty when unavailable).
    pub disks: Vec<IoSample>,
    /// Per network adapter, loopback excluded (empty when unavailable).
    pub nics: Vec<IoSample>,
}

type Shared = Arc<Mutex<Option<RawMetrics>>>;

pub struct Collector {
    shared: Shared,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    gpu_backend: Arc<Mutex<&'static str>>,
}

impl Collector {
    pub fn spawn(poll_hz: f32) -> Self {
        let shared: Shared = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let gpu_backend = Arc::new(Mutex::new("probing"));
        let interval = Duration::from_secs_f32(1.0 / poll_hz.clamp(0.5, 20.0))
            .max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);

        let (s, st, gb) = (shared.clone(), stop.clone(), gpu_backend.clone());
        let handle = thread::Builder::new()
            .name("platomon-collector".into())
            .spawn(move || run(s, st, gb, interval))
            .ok();

        Self { shared, stop, handle, gpu_backend }
    }

    /// Latest snapshot, if any has been published yet.
    pub fn latest(&self) -> Option<RawMetrics> {
        self.shared.lock().ok().and_then(|g| g.clone())
    }

    pub fn gpu_backend(&self) -> &'static str {
        self.gpu_backend.lock().map(|g| *g).unwrap_or("?")
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.thread().unpark();
            let _ = h.join();
        }
    }
}

fn run(shared: Shared, stop: Arc<AtomicBool>, gpu_backend: Arc<Mutex<&'static str>>, interval: Duration) {
    // Construction touches OS APIs; a panic must never take the app down.
    let Ok(mut sys) = catch_unwind(system::SystemSampler::new) else { return };
    let mut gpu = catch_unwind(gpu::GpuSampler::new).ok();
    if let Ok(mut g) = gpu_backend.lock() {
        *g = gpu.as_ref().map_or("none", |g| g.backend_name());
    }
    let mut sensors = catch_unwind(sensors::SensorSampler::new).ok();

    let mut seq = 0u64;
    let mut last = Instant::now();
    thread::park_timeout(interval);

    while !stop.load(Ordering::Relaxed) {
        let start = Instant::now();
        let elapsed = start.duration_since(last).as_secs_f64();
        last = start;

        let sampled = catch_unwind(AssertUnwindSafe(|| {
            let mut m = sys.sample(elapsed);
            let g = gpu.as_mut().and_then(|g| g.sample());
            if let Some(g) = g {
                m.gpu_util = Some(g.util);
                m.gpu_mem_util = g.mem_util;
            }
            if let Some(s) = sensors.as_mut() {
                m.sensors = merge_sensors(s.sample(), g, &m);
            }
            m
        }));
        let mut m = match sampled {
            Ok(m) => m,
            Err(_) => {
                // a backend that panicked once is not trusted again
                gpu = None;
                sensors = None;
                RawMetrics { elapsed, ..RawMetrics::default() }
            }
        };

        seq += 1;
        m.seq = seq;
        if let Ok(mut g) = shared.lock() {
            *g = Some(m);
        }

        let spent = start.elapsed();
        if spent < interval {
            thread::park_timeout(interval - spent);
        }
    }
}

/// The sensors' readings, with the GPU backend's (NVML: more direct than a
/// hardware monitor's) for the GPU and the system sampler's CPU clock
/// where the sensors have none.
fn merge_sensors(s: sensors::SensorSample, g: Option<gpu::GpuSample>, m: &RawMetrics) -> sensors::SensorSample {
    let mut out = s.or(m.sensors);
    if let Some(g) = g {
        out.gpu_temp = g.temp.or(out.gpu_temp);
        out.gpu_clock = g.clock.or(out.gpu_clock);
        out.gpu_clock_max = g.clock_max.or(out.gpu_clock_max);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_backend_readings_win_for_the_gpu() {
        let s = sensors::SensorSample { gpu_temp: Some(40.0), gpu_volt: Some(0.9), cpu_temp: Some(50.0), ..Default::default() };
        let g = gpu::GpuSample { util: 1.0, mem_util: None, temp: Some(45.0), clock: Some(1500.0), clock_max: None };
        let m = RawMetrics { sensors: sensors::SensorSample { cpu_clock: Some(3000.0), ..Default::default() }, ..Default::default() };
        let r = merge_sensors(s, Some(g), &m);
        assert_eq!((r.gpu_temp, r.gpu_clock, r.gpu_volt, r.cpu_temp, r.cpu_clock), (Some(45.0), Some(1500.0), Some(0.9), Some(50.0), Some(3000.0)));
    }

    #[test]
    fn io_sample_matching() {
        let d = IoSample { name: "Windows".into(), alt: Some(r"C:\".into()), ..Default::default() };
        assert!(d.matches("windows"));
        assert!(d.matches("C:"));
        assert!(d.matches(r"c:\"));
        assert!(!d.matches("D:"));
        assert!(!d.matches(""));
        let n = IoSample { name: "eth0".into(), ..Default::default() };
        assert!(n.matches("ETH0") && !n.matches("eth1"));
    }
}
