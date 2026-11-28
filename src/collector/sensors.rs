//! Temperatures, clocks and voltages of the CPU and GPU.
//!
//! * **Windows:** PDH counters give the CPU's clock (Processor Information:
//!   its nominal frequency times its performance) and a temperature (the
//!   hottest ACPI thermal zone). When LibreHardwareMonitor or
//!   OpenHardwareMonitor is running, their WMI sensors add the CPU package
//!   temperature, the core voltage and the GPU's temperature, clock and
//!   voltage. AMD Radeons report their own temperature, clock and voltage
//!   through the driver's ADL (Overdrive 8 on Navi and later, Overdrive 5/6
//!   on older GCN cards).
//! * **Linux:** sysfs thermal zones and hwmon (CPU temperature and core
//!   voltage; amdgpu temperature, clock and voltage).
//!
//! The GPU's own backend (NVML) fills in what is missing here. Anything
//! unavailable is `None`; nothing here may panic the collector.

/// One reading of every sensor.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SensorSample {
    /// °C
    pub cpu_temp: Option<f32>,
    /// MHz
    pub cpu_clock: Option<f32>,
    /// The CPU's nominal clock, MHz (boost goes past it).
    pub cpu_clock_base: Option<f32>,
    /// Core voltage, V.
    pub cpu_volt: Option<f32>,
    /// °C
    pub gpu_temp: Option<f32>,
    /// Core clock, MHz.
    pub gpu_clock: Option<f32>,
    /// The highest core clock the GPU reports it can reach, MHz.
    pub gpu_clock_max: Option<f32>,
    /// Core voltage, V.
    pub gpu_volt: Option<f32>,
}

impl SensorSample {
    /// Fill every field `self` lacks from `other`.
    pub fn or(self, other: SensorSample) -> SensorSample {
        SensorSample {
            cpu_temp: self.cpu_temp.or(other.cpu_temp),
            cpu_clock: self.cpu_clock.or(other.cpu_clock),
            cpu_clock_base: self.cpu_clock_base.or(other.cpu_clock_base),
            cpu_volt: self.cpu_volt.or(other.cpu_volt),
            gpu_temp: self.gpu_temp.or(other.gpu_temp),
            gpu_clock: self.gpu_clock.or(other.gpu_clock),
            gpu_clock_max: self.gpu_clock_max.or(other.gpu_clock_max),
            gpu_volt: self.gpu_volt.or(other.gpu_volt),
        }
    }
}

fn temp_ok(c: f32) -> Option<f32> {
    (c.is_finite() && (-20.0..150.0).contains(&c)).then_some(c)
}

fn volt_ok(v: f32) -> Option<f32> {
    (v.is_finite() && (0.2..3.0).contains(&v)).then_some(v)
}

fn clock_ok(mhz: f32) -> Option<f32> {
    (mhz.is_finite() && (1.0..20_000.0).contains(&mhz)).then_some(mhz)
}

/// One hardware-monitor sensor (LibreHardwareMonitor's WMI `Sensor`).
#[derive(Debug, Clone, PartialEq)]
pub struct HwSensor {
    pub name: String,
    /// `Temperature`, `Clock`, `Voltage`, ...
    pub kind: String,
    /// The hardware it belongs to: `/amdcpu/0`, `/intelcpu/0`, `/gpu-nvidia/0`, `/lpc/nct6798d`.
    pub parent: String,
    pub value: f32,
}

/// The readings in a hardware monitor's sensors: the CPU package (else
/// hottest core) temperature, the mean core clock, the core voltage (the
/// CPU's own, else the motherboard's Vcore), and the GPU core's
/// temperature, clock and voltage.
pub fn from_hw_monitor(sensors: &[HwSensor]) -> SensorSample {
    let lower: Vec<(String, String, String, f32)> = sensors
        .iter()
        .map(|s| (s.name.to_ascii_lowercase(), s.kind.to_ascii_lowercase(), s.parent.to_ascii_lowercase(), s.value))
        .collect();
    let of = |kind: &str, part: &dyn Fn(&str) -> bool| -> Vec<(&str, f32)> {
        lower.iter().filter(|(_, k, p, v)| k == kind && part(p) && v.is_finite()).map(|(n, _, _, v)| (n.as_str(), *v)).collect()
    };
    let cpu = |p: &str| p.contains("cpu");
    let gpu = |p: &str| p.contains("gpu");
    let board = |p: &str| p.contains("/lpc/") || p.contains("mainboard") || p.contains("motherboard");
    // the first sensor whose name matches a wanted name, in order of preference
    let prefer = |list: &[(&str, f32)], wanted: &[&dyn Fn(&str) -> bool]| -> Option<f32> {
        wanted.iter().find_map(|w| list.iter().find(|(n, _)| w(n)).map(|(_, v)| *v))
    };
    let max = |list: &[(&str, f32)]| list.iter().map(|(_, v)| *v).reduce(f32::max);

    // (Intel's "Distance to TjMax" sensors are temperatures too: headroom, not heat)
    let cpu_temps: Vec<(&str, f32)> = of("temperature", &cpu).into_iter().filter(|(n, _)| !n.contains("distance")).collect();
    let cpu_temp = prefer(&cpu_temps, &[&|n| n.contains("package"), &|n| n.contains("tctl") || n.contains("tdie")]).or_else(|| max(&cpu_temps));

    let cpu_clocks = of("clock", &cpu);
    let cores: Vec<f32> = cpu_clocks.iter().filter(|(n, _)| n.starts_with("core #") || n.starts_with("cpu core #")).map(|(_, v)| *v).collect();
    let cpu_clock = prefer(&cpu_clocks, &[&|n| n.contains("average") && !n.contains("effective")])
        .or_else(|| (!cores.is_empty()).then(|| cores.iter().sum::<f32>() / cores.len() as f32));

    let cpu_volts = of("voltage", &cpu);
    let board_volts = of("voltage", &board);
    let cpu_volt = prefer(
        &cpu_volts,
        &[&|n| n == "cpu core" || n == "core", &|n| n.starts_with("core") && n.contains("svi"), &|n| n.contains("core") && !n.contains("vid") && !n.contains("soc")],
    )
    .or_else(|| prefer(&board_volts, &[&|n| n.contains("vcore") || n == "cpu core"]))
    .or_else(|| prefer(&cpu_volts, &[&|n| n.contains("vid")]));

    let gpu_temps = of("temperature", &gpu);
    let gpu_temp = prefer(&gpu_temps, &[&|n| n == "gpu core"]).or_else(|| max(&gpu_temps));
    let gpu_clock = prefer(&of("clock", &gpu), &[&|n| n == "gpu core"]);
    let gpu_volt = prefer(&of("voltage", &gpu), &[&|n| n == "gpu core" || n.contains("core")]);

    SensorSample {
        cpu_temp: cpu_temp.and_then(temp_ok),
        cpu_clock: cpu_clock.and_then(clock_ok),
        cpu_clock_base: None,
        cpu_volt: cpu_volt.and_then(volt_ok),
        gpu_temp: gpu_temp.and_then(temp_ok),
        gpu_clock: gpu_clock.and_then(clock_ok),
        gpu_clock_max: None,
        gpu_volt: gpu_volt.and_then(volt_ok),
    }
}

/// Polls the platform's sensors.
pub struct SensorSampler {
    #[cfg(windows)]
    pdh: Option<win::Pdh>,
    #[cfg(windows)]
    wmi: Option<win::Wmi>,
    /// An AMD Radeon's driver.
    #[cfg(windows)]
    adl: Option<win::Adl>,
    /// When to look for a hardware monitor again, and when to next read it.
    #[cfg(windows)]
    next_connect: std::time::Instant,
    #[cfg(windows)]
    next_read: std::time::Instant,
    /// The last hardware-monitor reading (it is read less often).
    #[cfg(windows)]
    hw: SensorSample,
    #[cfg(windows)]
    hw_source: &'static str,
}

#[cfg(windows)]
const HW_READ_EVERY: std::time::Duration = std::time::Duration::from_millis(1000);
#[cfg(windows)]
const HW_CONNECT_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

impl SensorSampler {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            let now = std::time::Instant::now();
            Self { pdh: win::Pdh::new(), wmi: None, adl: win::Adl::new(), next_connect: now, next_read: now, hw: SensorSample::default(), hw_source: "" }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }

    /// Where the readings come from, for `--list sensors` and the F1 panel.
    pub fn sources(&self) -> String {
        #[cfg(windows)]
        {
            let mut s = vec![];
            if self.pdh.is_some() {
                s.push("PDH");
            }
            if self.adl.is_some() {
                s.push("AMD ADL");
            }
            if self.wmi.is_some() {
                s.push(self.hw_source);
            }
            if s.is_empty() { "none".into() } else { s.join(" + ") }
        }
        #[cfg(target_os = "linux")]
        {
            "sysfs".into()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            "none".into()
        }
    }

    pub fn sample(&mut self) -> SensorSample {
        #[cfg(windows)]
        {
            let now = std::time::Instant::now();
            if self.wmi.is_none() && now >= self.next_connect {
                self.next_connect = now + HW_CONNECT_EVERY;
                for (ns, name) in [(r"ROOT\LibreHardwareMonitor", "LibreHardwareMonitor"), (r"ROOT\OpenHardwareMonitor", "OpenHardwareMonitor")] {
                    if let Some(w) = win::Wmi::connect(ns) {
                        self.wmi = Some(w);
                        self.hw_source = name;
                        self.next_read = now;
                        break;
                    }
                }
            }
            if let Some(w) = &self.wmi
                && now >= self.next_read
            {
                self.next_read = now + HW_READ_EVERY;
                match w.sensors() {
                    Some(list) => self.hw = from_hw_monitor(&list),
                    // the monitor was closed: look for it again later
                    None => {
                        self.wmi = None;
                        self.hw = SensorSample::default();
                    }
                }
            }
            let pdh = self.pdh.as_mut().map(win::Pdh::sample).unwrap_or_default();
            let adl = self.adl.as_ref().map(win::Adl::sample).unwrap_or_default();
            // the GPU's driver first; the monitor's package temperature beats
            // the ACPI zone; PDH's clock is fresh every sample
            let s = adl.or(self.hw).or(pdh);
            SensorSample { cpu_clock: pdh.cpu_clock.or(self.hw.cpu_clock), ..s }
        }
        #[cfg(target_os = "linux")]
        {
            linux::sample()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            SensorSample::default()
        }
    }
}

impl Default for SensorSampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{SensorSample, clock_ok, temp_ok, volt_ok};
    use std::fs;
    use std::path::Path;

    fn read_f32(p: &Path) -> Option<f32> {
        fs::read_to_string(p).ok()?.trim().parse::<f32>().ok()
    }

    pub fn sample() -> SensorSample {
        let mut s = SensorSample::default();
        // CPU temperature: the package zone, else the hottest
        let mut best: Option<(bool, f32)> = None;
        if let Ok(rd) = fs::read_dir("/sys/class/thermal") {
            for e in rd.flatten() {
                let d = e.path();
                let Some(t) = read_f32(&d.join("temp")).and_then(|m| temp_ok(m / 1000.0)) else { continue };
                let ty = fs::read_to_string(d.join("type")).unwrap_or_default().to_ascii_lowercase();
                let pkg = ty.contains("pkg") || ty.contains("cpu");
                if best.is_none_or(|(bp, bt)| (pkg && !bp) || (pkg == bp && t > bt)) {
                    best = Some((pkg, t));
                }
            }
        }
        s.cpu_temp = best.map(|b| b.1);
        // hwmon: k10temp / coretemp temperatures, Vcore, amdgpu
        if let Ok(rd) = fs::read_dir("/sys/class/hwmon") {
            for e in rd.flatten() {
                let d = e.path();
                let name = fs::read_to_string(d.join("name")).unwrap_or_default().trim().to_string();
                match name.as_str() {
                    "k10temp" | "coretemp" | "zenpower" => {
                        if let Some(t) = read_f32(&d.join("temp1_input")).and_then(|m| temp_ok(m / 1000.0)) {
                            s.cpu_temp = Some(t);
                        }
                    }
                    "amdgpu" => {
                        s.gpu_temp = s.gpu_temp.or_else(|| read_f32(&d.join("temp1_input")).and_then(|m| temp_ok(m / 1000.0)));
                        s.gpu_clock = s.gpu_clock.or_else(|| read_f32(&d.join("freq1_input")).and_then(|hz| clock_ok(hz / 1e6)));
                        s.gpu_volt = s.gpu_volt.or_else(|| read_f32(&d.join("in0_input")).and_then(|mv| volt_ok(mv / 1000.0)));
                    }
                    _ => {
                        for i in 0..16 {
                            let label = fs::read_to_string(d.join(format!("in{i}_label"))).unwrap_or_default().to_ascii_lowercase();
                            if label.contains("vcore") || label.trim() == "cpu core" {
                                s.cpu_volt = s.cpu_volt.or_else(|| read_f32(&d.join(format!("in{i}_input"))).and_then(|mv| volt_ok(mv / 1000.0)));
                            }
                        }
                    }
                }
            }
        }
        s
    }
}

#[cfg(windows)]
mod win {
    use super::{HwSensor, SensorSample, clock_ok, temp_ok};
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    // ------------------------------------------------------------ PDH

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct FmtValue {
        status: u32,
        _pad: u32,
        value: f64,
    }

    #[repr(C)]
    struct FmtItem {
        name: *mut u16,
        value: FmtValue,
    }

    const PDH_FMT_DOUBLE: u32 = 0x0000_0200;
    const PDH_FMT_NOCAP100: u32 = 0x0000_8000;
    const PDH_MORE_DATA: u32 = 0x8000_07D2;
    /// PDH_CSTATUS_VALID_DATA and PDH_CSTATUS_NEW_DATA.
    const PDH_VALID: u32 = 1;

    #[link(name = "pdh")]
    unsafe extern "system" {
        fn PdhOpenQueryW(source: *const u16, user: usize, query: *mut isize) -> u32;
        fn PdhAddEnglishCounterW(query: isize, path: *const u16, user: usize, counter: *mut isize) -> u32;
        fn PdhCollectQueryData(query: isize) -> u32;
        fn PdhGetFormattedCounterValue(counter: isize, format: u32, kind: *mut u32, value: *mut FmtValue) -> u32;
        fn PdhGetFormattedCounterArrayW(counter: isize, format: u32, size: *mut u32, count: *mut u32, items: *mut FmtItem) -> u32;
        fn PdhCloseQuery(query: isize) -> u32;
    }

    /// The CPU's clock and the thermal zones' temperatures, from PDH.
    pub struct Pdh {
        query: isize,
        perf: Option<isize>,
        freq: Option<isize>,
        zones: Option<isize>,
        /// `zones` is High Precision Temperature (tenths of a kelvin).
        tenths: bool,
    }

    impl Pdh {
        pub fn new() -> Option<Self> {
            let mut query = 0;
            // SAFETY: valid out pointer; a null source means live data.
            if unsafe { PdhOpenQueryW(null(), 0, &mut query) } != 0 {
                return None;
            }
            let add = |path: &str| {
                let w = wide(path);
                let mut c = 0;
                // SAFETY: an open query, a NUL-terminated path, a valid out pointer.
                (unsafe { PdhAddEnglishCounterW(query, w.as_ptr(), 0, &mut c) } == 0).then_some(c)
            };
            let perf = add(r"\Processor Information(_Total)\% Processor Performance");
            let freq = add(r"\Processor Information(_Total)\Processor Frequency");
            let (zones, tenths) = match add(r"\Thermal Zone Information(*)\High Precision Temperature") {
                Some(c) => (Some(c), true),
                None => (add(r"\Thermal Zone Information(*)\Temperature"), false),
            };
            // the first collection primes the rate counters
            // SAFETY: an open query.
            unsafe { PdhCollectQueryData(query) };
            Some(Self { query, perf, freq, zones, tenths })
        }

        fn value(c: isize) -> Option<f64> {
            let mut v = FmtValue::default();
            // SAFETY: a counter of our query; valid out pointers.
            let r = unsafe { PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE | PDH_FMT_NOCAP100, null_mut(), &mut v) };
            (r == 0 && v.status <= PDH_VALID && v.value.is_finite()).then_some(v.value)
        }

        /// The largest of a wildcard counter's instances.
        fn array_max(c: isize) -> Option<f64> {
            let (mut size, mut count) = (0u32, 0u32);
            // SAFETY: asks for the buffer size (no buffer yet).
            let r = unsafe { PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, null_mut()) };
            if r != PDH_MORE_DATA || size == 0 {
                return None;
            }
            // 8-byte aligned, at least `size` bytes
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            // SAFETY: `buf` holds `size` bytes, aligned for FmtItem.
            let r = unsafe { PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, buf.as_mut_ptr() as *mut FmtItem) };
            if r != 0 {
                return None;
            }
            // SAFETY: PDH wrote `count` items at the start of `buf`.
            let items = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const FmtItem, count as usize) };
            items.iter().filter(|i| i.value.status <= PDH_VALID && i.value.value.is_finite()).map(|i| i.value.value).reduce(f64::max)
        }

        pub fn sample(&mut self) -> SensorSample {
            // SAFETY: an open query.
            unsafe { PdhCollectQueryData(self.query) };
            let mut s = SensorSample::default();
            if let (Some(p), Some(f)) = (self.perf.and_then(Self::value), self.freq.and_then(Self::value))
                && f > 0.0
            {
                s.cpu_clock = clock_ok((f * p / 100.0) as f32);
                s.cpu_clock_base = clock_ok(f as f32);
            }
            if let Some(t) = self.zones.and_then(Self::array_max) {
                let kelvin = if self.tenths { t / 10.0 } else { t };
                s.cpu_temp = temp_ok((kelvin - 273.15) as f32);
            }
            s
        }
    }

    impl Drop for Pdh {
        fn drop(&mut self) {
            // SAFETY: our open query, closed once.
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    // ------------------------------------------------------------ AMD ADL

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
        fn GetProcessHeap() -> *mut c_void;
        fn HeapAlloc(heap: *mut c_void, flags: u32, bytes: usize) -> *mut c_void;
    }

    /// ADL's allocator callback (what it hands back is small and rare).
    extern "system" fn adl_alloc(size: i32) -> *mut c_void {
        // SAFETY: the process heap, a plain allocation.
        unsafe { HeapAlloc(GetProcessHeap(), 0x8, size.max(1) as usize) }
    }

    /// ADL's AdapterInfo (Windows layout).
    #[repr(C)]
    struct AdapterInfo {
        size: i32,
        index: i32,
        udid: [u8; 256],
        bus: i32,
        device: i32,
        function: i32,
        vendor: i32,
        name: [u8; 256],
        display: [u8; 256],
        present: i32,
        exist: i32,
        driver_path: [u8; 256],
        driver_path_ext: [u8; 256],
        pnp: [u8; 256],
        os_display: i32,
    }

    /// ADLPMLogDataOutput: per sensor (supported, value).
    #[repr(C)]
    struct PmLog {
        size: i32,
        sensors: [[i32; 2]; 256],
    }

    /// ADLPMActivity (Overdrive 5).
    #[repr(C)]
    #[derive(Default)]
    struct PmActivity {
        size: i32,
        engine_clock: i32,
        memory_clock: i32,
        vddc: i32,
        activity: i32,
        level: i32,
        bus_speed: i32,
        bus_lanes: i32,
        max_bus_lanes: i32,
        reserved: i32,
    }

    // ADLSensorType indices
    const PMLOG_CLK_GFXCLK: usize = 1;
    const PMLOG_TEMPERATURE_EDGE: usize = 8;
    const PMLOG_GFX_VOLTAGE: usize = 21;

    type Create = unsafe extern "C" fn(extern "system" fn(i32) -> *mut c_void, i32, *mut *mut c_void) -> i32;
    type Destroy = unsafe extern "C" fn(*mut c_void) -> i32;
    type Count = unsafe extern "C" fn(*mut c_void, *mut i32) -> i32;
    type Infos = unsafe extern "C" fn(*mut c_void, *mut AdapterInfo, i32) -> i32;
    type Active = unsafe extern "C" fn(*mut c_void, i32, *mut i32) -> i32;
    type Caps = unsafe extern "C" fn(*mut c_void, i32, *mut i32, *mut i32, *mut i32) -> i32;
    type PmLogGet = unsafe extern "C" fn(*mut c_void, i32, *mut PmLog) -> i32;
    type Od5Activity = unsafe extern "C" fn(*mut c_void, i32, *mut PmActivity) -> i32;
    type Od5Temp = unsafe extern "C" fn(*mut c_void, i32, i32, *mut [i32; 2]) -> i32;

    enum Od {
        /// Overdrive 8 (Navi and later): the PM log.
        Pm(PmLogGet),
        /// Overdrive 5/6 (GCN, the HD 7000s on): current activity and temperature.
        Five(Od5Activity, Option<Od5Temp>),
    }

    /// An AMD Radeon's temperature, clock and voltage through the driver's ADL.
    pub struct Adl {
        lib: *mut c_void,
        ctx: *mut c_void,
        destroy: Destroy,
        adapter: i32,
        od: Od,
    }

    impl Adl {
        pub fn new() -> Option<Self> {
            // SAFETY (whole fn): the driver's own library; each export is
            // looked up by name and called with its ADL signature.
            unsafe {
                let lib = LoadLibraryW(wide("atiadlxx.dll").as_ptr());
                if lib.is_null() {
                    return None;
                }
                let sym = |n: &str| {
                    let c: Vec<u8> = n.bytes().chain(std::iter::once(0)).collect();
                    let p = GetProcAddress(lib, c.as_ptr());
                    (!p.is_null()).then_some(p)
                };
                let found = (|| {
                    let create: Create = std::mem::transmute(sym("ADL2_Main_Control_Create")?);
                    let destroy: Destroy = std::mem::transmute(sym("ADL2_Main_Control_Destroy")?);
                    let count: Count = std::mem::transmute(sym("ADL2_Adapter_NumberOfAdapters_Get")?);
                    let infos: Infos = std::mem::transmute(sym("ADL2_Adapter_AdapterInfo_Get")?);
                    let active: Option<Active> = sym("ADL2_Adapter_Active_Get").map(|p| std::mem::transmute(p));
                    let caps: Caps = std::mem::transmute(sym("ADL2_Overdrive_Caps")?);
                    let mut ctx = null_mut();
                    if create(adl_alloc, 1, &mut ctx) != 0 || ctx.is_null() {
                        return None;
                    }
                    let mut n = 0;
                    let fail = |ctx| {
                        destroy(ctx);
                        None
                    };
                    if count(ctx, &mut n) != 0 || n <= 0 || n > 256 {
                        return fail(ctx);
                    }
                    let mut list: Vec<AdapterInfo> = (0..n).map(|_| std::mem::zeroed()).collect();
                    for a in &mut list {
                        a.size = size_of::<AdapterInfo>() as i32;
                    }
                    if infos(ctx, list.as_mut_ptr(), (n as usize * size_of::<AdapterInfo>()) as i32) != 0 {
                        return fail(ctx);
                    }
                    // the first active AMD adapter whose Overdrive we can read (ADL gives the vendor id in decimal)
                    for a in list.iter().filter(|a| a.vendor == 1002 || a.vendor == 0x1002) {
                        let mut on = 1;
                        if let Some(f) = active
                            && (f(ctx, a.index, &mut on) != 0 || on == 0)
                        {
                            continue;
                        }
                        let (mut sup, mut en, mut ver) = (0, 0, 0);
                        if caps(ctx, a.index, &mut sup, &mut en, &mut ver) != 0 || sup == 0 {
                            continue;
                        }
                        let od = if ver >= 8 {
                            sym("ADL2_New_QueryPMLogData_Get").map(|p| Od::Pm(std::mem::transmute::<*mut c_void, PmLogGet>(p)))
                        } else {
                            sym("ADL2_Overdrive5_CurrentActivity_Get").map(|p| {
                                Od::Five(std::mem::transmute::<*mut c_void, Od5Activity>(p), sym("ADL2_Overdrive5_Temperature_Get").map(|t| std::mem::transmute(t)))
                            })
                        };
                        if let Some(od) = od {
                            return Some((ctx, destroy, a.index, od));
                        }
                    }
                    fail(ctx)
                })();
                match found {
                    Some((ctx, destroy, adapter, od)) => Some(Self { lib, ctx, destroy, adapter, od }),
                    None => {
                        FreeLibrary(lib);
                        None
                    }
                }
            }
        }

        pub fn sample(&self) -> SensorSample {
            let mut s = SensorSample::default();
            // SAFETY: functions of the loaded library, our context and
            // adapter, buffers sized as ADL expects.
            unsafe {
                match self.od {
                    Od::Pm(get) => {
                        let mut log: PmLog = std::mem::zeroed();
                        log.size = size_of::<PmLog>() as i32;
                        if get(self.ctx, self.adapter, &mut log) == 0 {
                            let at = |i: usize| (log.sensors[i][0] != 0).then_some(log.sensors[i][1] as f32);
                            s.gpu_clock = at(PMLOG_CLK_GFXCLK).and_then(clock_ok);
                            s.gpu_temp = at(PMLOG_TEMPERATURE_EDGE).and_then(temp_ok);
                            s.gpu_volt = at(PMLOG_GFX_VOLTAGE).and_then(|mv| super::volt_ok(mv / 1000.0));
                        }
                    }
                    Od::Five(activity, temp) => {
                        let mut a = PmActivity { size: size_of::<PmActivity>() as i32, ..Default::default() };
                        if activity(self.ctx, self.adapter, &mut a) == 0 {
                            // (engine clock in 10 kHz)
                            s.gpu_clock = clock_ok(a.engine_clock as f32 / 100.0);
                            s.gpu_volt = super::volt_ok(a.vddc as f32 / 1000.0);
                        }
                        let mut t = [size_of::<[i32; 2]>() as i32, 0];
                        if let Some(f) = temp
                            && f(self.ctx, self.adapter, 0, &mut t) == 0
                        {
                            // (millidegrees)
                            s.gpu_temp = temp_ok(t[1] as f32 / 1000.0);
                        }
                    }
                }
            }
            s
        }
    }

    impl Drop for Adl {
        fn drop(&mut self) {
            // SAFETY: our context and library, released once.
            unsafe {
                (self.destroy)(self.ctx);
                FreeLibrary(self.lib);
            }
        }
    }

    // ------------------------------------------------------------ WMI

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);

    const CLSID_WBEM_LOCATOR: Guid = Guid(0x4590_f811, 0x1d3a, 0x11d0, [0x89, 0x1f, 0x00, 0xaa, 0x00, 0x4b, 0x2e, 0x24]);
    const IID_IWBEM_LOCATOR: Guid = Guid(0xdc12_a687, 0x737f, 0x11cf, [0x88, 0x4d, 0x00, 0xaa, 0x00, 0x4b, 0x2e, 0x24]);
    const CLSCTX_INPROC_SERVER: u32 = 1;
    const COINIT_MULTITHREADED: u32 = 0;
    const RPC_C_AUTHN_WINNT: u32 = 10;
    const RPC_C_AUTHN_LEVEL_CALL: u32 = 3;
    const RPC_C_IMP_LEVEL_IMPERSONATE: u32 = 3;
    const WBEM_FLAG_FORWARD_ONLY_RETURN_IMMEDIATELY: i32 = 0x30;
    const VT_BSTR: u16 = 8;
    const VT_R4: u16 = 4;
    const VT_R8: u16 = 5;

    #[repr(C)]
    struct Variant {
        vt: u16,
        _reserved: [u16; 3],
        data: [u64; 2],
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoCreateInstance(clsid: *const Guid, outer: *mut c_void, ctx: u32, iid: *const Guid, out: *mut *mut c_void) -> i32;
        fn CoSetProxyBlanket(
            proxy: *mut c_void,
            authn: u32,
            authz: u32,
            principal: *const u16,
            level: u32,
            impersonation: u32,
            identity: *mut c_void,
            caps: u32,
        ) -> i32;
    }

    #[link(name = "oleaut32")]
    unsafe extern "system" {
        fn SysAllocString(s: *const u16) -> *mut u16;
        fn SysFreeString(s: *mut u16);
        fn VariantClear(v: *mut Variant) -> i32;
    }

    struct Bstr(*mut u16);

    impl Bstr {
        fn new(s: &str) -> Self {
            let w = wide(s);
            // SAFETY: a NUL-terminated string.
            Self(unsafe { SysAllocString(w.as_ptr()) })
        }
    }

    impl Drop for Bstr {
        fn drop(&mut self) {
            // SAFETY: allocated by SysAllocString (null is allowed).
            unsafe { SysFreeString(self.0) };
        }
    }

    /// Method `i` of a COM object's vtable.
    ///
    /// # Safety
    /// `obj` is a live COM interface with at least `i + 1` methods.
    unsafe fn method(obj: *mut c_void, i: usize) -> *const c_void {
        // SAFETY: per the caller; a COM object starts with its vtable pointer.
        unsafe { *(*(obj as *const *const *const c_void)).add(i) }
    }

    /// # Safety
    /// `obj` is a live COM interface this code holds a reference to.
    unsafe fn release(obj: *mut c_void) {
        if !obj.is_null() {
            // SAFETY: IUnknown::Release is method 2 of every interface.
            unsafe {
                let f: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(method(obj, 2));
                f(obj);
            }
        }
    }

    /// A connection to a hardware monitor's WMI namespace.
    pub struct Wmi {
        services: *mut c_void,
    }

    impl Wmi {
        pub fn connect(namespace: &str) -> Option<Self> {
            // SAFETY (whole fn): COM calls with valid pointers; the
            // vtable slots are IWbemLocator::ConnectServer (3).
            unsafe {
                // (already initialised on this thread is fine)
                CoInitializeEx(null_mut(), COINIT_MULTITHREADED);
                let mut locator = null_mut();
                if CoCreateInstance(&CLSID_WBEM_LOCATOR, null_mut(), CLSCTX_INPROC_SERVER, &IID_IWBEM_LOCATOR, &mut locator) < 0 || locator.is_null() {
                    return None;
                }
                type Connect = unsafe extern "system" fn(
                    *mut c_void,
                    *mut u16,
                    *mut u16,
                    *mut u16,
                    *mut u16,
                    i32,
                    *mut u16,
                    *mut c_void,
                    *mut *mut c_void,
                ) -> i32;
                let connect: Connect = std::mem::transmute(method(locator, 3));
                let ns = Bstr::new(namespace);
                let mut services = null_mut();
                let hr = connect(locator, ns.0, null_mut(), null_mut(), null_mut(), 0, null_mut(), null_mut(), &mut services);
                release(locator);
                if hr < 0 || services.is_null() {
                    return None;
                }
                CoSetProxyBlanket(services, RPC_C_AUTHN_WINNT, 0, null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, null_mut(), 0);
                let me = Self { services };
                // a namespace with no sensors (the monitor's leftovers) is no monitor
                me.sensors().filter(|s| !s.is_empty()).map(|_| me)
            }
        }

        /// Every sensor's name, type, hardware and value; `None` when the
        /// query fails (the monitor has closed).
        pub fn sensors(&self) -> Option<Vec<HwSensor>> {
            // SAFETY (whole fn): COM calls on live interfaces; vtable slots:
            // IWbemServices::ExecQuery (20), IEnumWbemClassObject::Next (4),
            // IWbemClassObject::Get (4).
            unsafe {
                type Exec = unsafe extern "system" fn(*mut c_void, *mut u16, *mut u16, i32, *mut c_void, *mut *mut c_void) -> i32;
                type Next = unsafe extern "system" fn(*mut c_void, i32, u32, *mut *mut c_void, *mut u32) -> i32;
                let exec: Exec = std::mem::transmute(method(self.services, 20));
                let (wql, q) = (Bstr::new("WQL"), Bstr::new("SELECT Name, SensorType, Parent, Value FROM Sensor"));
                let mut en = null_mut();
                if exec(self.services, wql.0, q.0, WBEM_FLAG_FORWARD_ONLY_RETURN_IMMEDIATELY, null_mut(), &mut en) < 0 || en.is_null() {
                    return None;
                }
                let next: Next = std::mem::transmute(method(en, 4));
                let mut out = Vec::new();
                for _ in 0..4096 {
                    let (mut obj, mut n) = (null_mut(), 0u32);
                    if next(en, 2000, 1, &mut obj, &mut n) < 0 || n == 0 || obj.is_null() {
                        break;
                    }
                    let s = HwSensor {
                        name: get_string(obj, "Name").unwrap_or_default(),
                        kind: get_string(obj, "SensorType").unwrap_or_default(),
                        parent: get_string(obj, "Parent").unwrap_or_default(),
                        value: get_number(obj, "Value").unwrap_or(f32::NAN),
                    };
                    release(obj);
                    out.push(s);
                }
                release(en);
                Some(out)
            }
        }
    }

    impl Drop for Wmi {
        fn drop(&mut self) {
            // SAFETY: our reference, released once.
            unsafe { release(self.services) };
        }
    }

    /// # Safety
    /// `obj` is a live IWbemClassObject.
    unsafe fn get<T>(obj: *mut c_void, name: &str, read: impl FnOnce(&Variant) -> Option<T>) -> Option<T> {
        type Get = unsafe extern "system" fn(*mut c_void, *const u16, i32, *mut Variant, *mut i32, *mut i32) -> i32;
        let w = wide(name);
        let mut v = Variant { vt: 0, _reserved: [0; 3], data: [0; 2] };
        // SAFETY: per the caller; IWbemClassObject::Get is method 4.
        unsafe {
            let f: Get = std::mem::transmute(method(obj, 4));
            if f(obj, w.as_ptr(), 0, &mut v, null_mut(), null_mut()) < 0 {
                return None;
            }
            let r = read(&v);
            VariantClear(&mut v);
            r
        }
    }

    /// # Safety
    /// `obj` is a live IWbemClassObject.
    unsafe fn get_string(obj: *mut c_void, name: &str) -> Option<String> {
        // SAFETY: per the caller; a VT_BSTR holds a NUL-terminated string.
        unsafe {
            get(obj, name, |v| {
                if v.vt != VT_BSTR || v.data[0] == 0 {
                    return None;
                }
                let p = v.data[0] as *const u16;
                let mut len = 0;
                while *p.add(len) != 0 && len < 4096 {
                    len += 1;
                }
                Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)))
            })
        }
    }

    /// # Safety
    /// `obj` is a live IWbemClassObject.
    unsafe fn get_number(obj: *mut c_void, name: &str) -> Option<f32> {
        // SAFETY: per the caller.
        unsafe {
            get(obj, name, |v| match v.vt {
                VT_R4 => Some(f32::from_bits(v.data[0] as u32)),
                VT_R8 => Some(f64::from_bits(v.data[0]) as f32),
                _ => None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, kind: &str, parent: &str, value: f32) -> HwSensor {
        HwSensor { name: name.into(), kind: kind.into(), parent: parent.into(), value }
    }

    #[test]
    fn hardware_monitor_sensors_pick_the_cpu_and_gpu_core() {
        let intel = [
            s("Core #1", "Temperature", "/intelcpu/0", 61.0),
            s("Core #1 Distance to TjMax", "Temperature", "/intelcpu/0", 39.0),
            s("CPU Package", "Temperature", "/intelcpu/0", 64.0),
            s("Core #1", "Clock", "/intelcpu/0", 4000.0),
            s("Core #2", "Clock", "/intelcpu/0", 4400.0),
            s("Bus Speed", "Clock", "/intelcpu/0", 100.0),
            s("CPU Core", "Voltage", "/intelcpu/0", 1.21),
            s("Core #1 VID", "Voltage", "/intelcpu/0", 1.30),
            s("GPU Core", "Temperature", "/gpu-nvidia/0", 48.0),
            s("GPU Hot Spot", "Temperature", "/gpu-nvidia/0", 57.0),
            s("GPU Core", "Clock", "/gpu-nvidia/0", 1785.0),
            s("GPU Memory", "Clock", "/gpu-nvidia/0", 7000.0),
        ];
        let r = from_hw_monitor(&intel);
        assert_eq!(r.cpu_temp, Some(64.0), "package, not a core or the TjMax distance");
        assert_eq!(r.cpu_clock, Some(4200.0), "mean of the cores, not the bus");
        assert_eq!(r.cpu_volt, Some(1.21), "the core voltage, not the VID");
        assert_eq!((r.gpu_temp, r.gpu_clock, r.gpu_volt), (Some(48.0), Some(1785.0), None));

        let amd = [
            s("Core #1", "Temperature", "/amdcpu/0", 55.0),
            s("Core (Tctl/Tdie)", "Temperature", "/amdcpu/0", 70.0),
            s("Core #1 VID", "Voltage", "/amdcpu/0", 1.35),
            s("SoC (SVI2 TFN)", "Voltage", "/amdcpu/0", 1.10),
            s("Core (SVI2 TFN)", "Voltage", "/amdcpu/0", 1.25),
            s("Vcore", "Voltage", "/lpc/nct6798d", 1.28),
            s("GPU Core", "Voltage", "/gpu-amd/0", 0.95),
            s("GPU Core", "Temperature", "/gpu-amd/0", 150.5),
        ];
        let r = from_hw_monitor(&amd);
        assert_eq!(r.cpu_temp, Some(70.0));
        assert_eq!(r.cpu_volt, Some(1.25), "SVI2 core, not the SoC or VID");
        assert_eq!(r.gpu_volt, Some(0.95));
        assert_eq!(r.gpu_temp, None, "out of range: not trusted");
        assert_eq!(r.cpu_clock, None);

        // no CPU voltage of its own: the motherboard's Vcore
        let board = [s("Vcore", "Voltage", "/lpc/it8686e", 1.19), s("+12V", "Voltage", "/lpc/it8686e", 12.1)];
        assert_eq!(from_hw_monitor(&board).cpu_volt, Some(1.19));
        assert_eq!(from_hw_monitor(&[]), SensorSample::default());
    }

    #[test]
    fn samples_fill_gaps_from_each_other() {
        let a = SensorSample { cpu_temp: Some(50.0), ..Default::default() };
        let b = SensorSample { cpu_temp: Some(30.0), gpu_clock: Some(1500.0), ..Default::default() };
        let c = a.or(b);
        assert_eq!((c.cpu_temp, c.gpu_clock), (Some(50.0), Some(1500.0)));
    }

    /// Reads this machine's sensors (prints them; passes whatever they are).
    #[test]
    fn live_sensors_read_without_panicking() {
        let mut s = SensorSampler::new();
        let first = s.sample();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let second = s.sample();
        eprintln!("sources {}: {first:?}\n{second:?}", s.sources());
    }
}
