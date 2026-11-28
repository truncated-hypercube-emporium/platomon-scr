//! Key handling and the stats bar.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::metrics::{MetricKind, Reading};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    ToggleStats,
    ToggleLegend,
    ToggleWireframe,
    ToggleTags,
    /// F1: the debug panel.
    ToggleDebug,
    Resize(u16, u16),
}

pub fn map_event(ev: &Event) -> Action {
    match ev {
        Event::Key(KeyEvent { code, modifiers, kind, .. }) => {
            // Windows reports both press and release; act on press only.
            if *kind == KeyEventKind::Release {
                return Action::None;
            }
            match code {
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => Action::Quit,
                KeyCode::Char('s') | KeyCode::Char('S') if *kind == KeyEventKind::Press => Action::ToggleStats,
                KeyCode::Char('l') | KeyCode::Char('L') if *kind == KeyEventKind::Press => Action::ToggleLegend,
                KeyCode::Char('w') | KeyCode::Char('W') if *kind == KeyEventKind::Press => Action::ToggleWireframe,
                KeyCode::Char('t') | KeyCode::Char('T') if *kind == KeyEventKind::Press => Action::ToggleTags,
                KeyCode::F(1) if *kind == KeyEventKind::Press => Action::ToggleDebug,
                _ => Action::None,
            }
        }
        Event::Resize(w, h) => Action::Resize(*w, *h),
        _ => Action::None,
    }
}

/// Screensaver wake-up: any key press, mouse button or wheel, or the mouse
/// moving a couple of cells. Movement in the first second (the console
/// settling, a jittery mouse) only sets the reference position.
pub struct InputWatch {
    start: std::time::Instant,
    origin: Option<(u16, u16)>,
}

impl Default for InputWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl InputWatch {
    pub const GRACE: std::time::Duration = std::time::Duration::from_secs(1);
    /// Cells the pointer must travel to count as movement.
    pub const MOVE_CELLS: u16 = 2;

    pub fn new() -> Self {
        Self { start: std::time::Instant::now(), origin: None }
    }

    /// Start a new grace period and forget the reference position, e.g. after
    /// the window was moved or resized (the pointer's cell position changes
    /// without the mouse moving).
    pub fn rearm(&mut self) {
        *self = Self::new();
    }

    pub fn wakes(&mut self, ev: &Event) -> bool {
        self.wakes_at(ev, self.start.elapsed())
    }

    fn wakes_at(&mut self, ev: &Event, since_start: std::time::Duration) -> bool {
        use crossterm::event::{MouseEvent, MouseEventKind};
        match ev {
            Event::Key(k) => k.kind != KeyEventKind::Release,
            Event::Mouse(MouseEvent { kind: MouseEventKind::Moved, column, row, .. }) => {
                let here = (*column, *row);
                match self.origin {
                    Some(o) if since_start >= Self::GRACE => {
                        here.0.abs_diff(o.0) >= Self::MOVE_CELLS || here.1.abs_diff(o.1) >= Self::MOVE_CELLS
                    }
                    _ => {
                        self.origin = Some(here);
                        false
                    }
                }
            }
            Event::Mouse(_) => since_start >= Self::GRACE,
            _ => false,
        }
    }
}

fn pct(v: Option<f32>) -> String {
    v.map_or_else(|| "n/a".to_string(), |p| format!("{:.0}%", p))
}

fn rate(v: Option<f64>) -> String {
    v.map_or_else(|| "n/a".to_string(), |r| format!("{:.1}", r))
}

/// The CPU's label: `CPU`, or `CPU3` for one core.
fn cpu_label(r: &Reading) -> String {
    format!("CPU{}", r.name.as_deref().unwrap_or(""))
}

/// A sensor's value: `64°C`, `4.20 GHz`, `1.21 V` (`compact`: `64C`, `4.2G`, `1.21V`).
fn sensor(r: &Reading, compact: bool) -> String {
    let Some(v) = r.value else { return "n/a".into() };
    match (r.kind, compact) {
        (MetricKind::CpuTemp | MetricKind::GpuTemp, false) => format!("{v:.0}\u{b0}C"),
        (MetricKind::CpuTemp | MetricKind::GpuTemp, true) => format!("{v:.0}C"),
        (MetricKind::CpuFreq | MetricKind::GpuFreq, false) => format!("{:.2} GHz", v / 1000.0),
        (MetricKind::CpuFreq | MetricKind::GpuFreq, true) => format!("{:.1}G", v / 1000.0),
        (_, false) => format!("{v:.2} V"),
        (_, true) => format!("{v:.2}V"),
    }
}

/// A sensor's label: `CPU`/`GPU` and what it measures.
fn sensor_label(k: MetricKind, compact: bool) -> &'static str {
    match (k, compact) {
        (MetricKind::CpuTemp, _) => "CPU T",
        (MetricKind::GpuTemp, _) => "GPU T",
        (MetricKind::CpuFreq, _) => "CPU",
        (MetricKind::GpuFreq, _) => "GPU",
        (MetricKind::CpuVolt, false) => "CPU Vcore",
        (MetricKind::GpuVolt, false) => "GPU Vcore",
        (MetricKind::CpuVolt, true) => "CPU",
        _ => "GPU",
    }
}

/// One stats-bar segment.
pub fn segment(r: &Reading) -> String {
    let name = || r.name.clone().unwrap_or_else(|| "auto".into());
    let pair = |label: String, up: &str, down: &str| match (r.a, r.b) {
        (Some(_), Some(_)) => format!("{label} {up}{} {down}{} MB/s", rate(r.a), rate(r.b)),
        _ => format!("{label} n/a"),
    };
    match r.kind {
        k if k.is_sensor() => format!("{} {}", sensor_label(k, false), sensor(r, false)),
        MetricKind::Cpu => format!("{} {}", cpu_label(r), pct(r.pct)),
        MetricKind::Ram => format!("RAM {}", pct(r.pct)),
        MetricKind::Gpu => format!("GPU {}", pct(r.pct)),
        MetricKind::Disk => pair("DSK".into(), "r", "w"),
        MetricKind::Net => pair("NET".into(), "↓", "↑"),
        MetricKind::Nic => pair(format!("NIC {}", name()), "↓", "↑"),
        MetricKind::DiskDev => pair(format!("DEV {}", name()), "r", "w"),
        _ => String::new(),
    }
}

/// Compact legend label for block letters: `CPU 37%`, `NET 1.5M`,
/// `NIC ETHERNET 0.2M` (rates are read + write, MB/s).
pub fn short_label(r: &Reading) -> String {
    let total = || match (r.a, r.b) {
        (Some(a), Some(b)) => format!("{:.1}M", a + b),
        _ => "n/a".to_string(),
    };
    let name = || r.name.as_deref().unwrap_or("auto").chars().take(10).collect::<String>();
    match r.kind {
        k if k.is_sensor() => format!("{} {}", sensor_label(k, true), sensor(r, true)),
        MetricKind::Cpu => format!("{} {}", cpu_label(r), pct(r.pct)),
        MetricKind::Ram => format!("RAM {}", pct(r.pct)),
        MetricKind::Gpu => format!("GPU {}", pct(r.pct)),
        MetricKind::Disk => format!("DSK {}", total()),
        MetricKind::Net => format!("NET {}", total()),
        MetricKind::Nic => format!("NIC {} {}", name(), total()),
        MetricKind::DiskDev => format!("DEV {} {}", name(), total()),
        _ => String::new(),
    }
}

/// `CPU 37% | RAM 62% | DSK r12.4 w3.1 MB/s | NET ↓1.2 ↑0.3 MB/s | GPU 8% | fps 48/52`
pub fn stats_line(readings: &[&Reading], achieved_fps: f32, target_fps: f32) -> String {
    stats_line_fit(readings, achieved_fps, target_fps, "", usize::MAX)
}

/// Like [`stats_line`] (plus `extra` after the fps), fitted to `width`
/// characters: when too long, the metric part is cut (marked `~`) so the
/// fps reading always stays visible.
pub fn stats_line_fit(readings: &[&Reading], achieved_fps: f32, target_fps: f32, extra: &str, width: usize) -> String {
    let metrics = readings.iter().map(|r| segment(r)).collect::<Vec<_>>().join(" | ");
    let fps = format!("fps {achieved_fps:.0}/{target_fps:.0}{extra}");
    let full = if metrics.is_empty() { fps.clone() } else { format!("{metrics} | {fps}") };
    let len = |s: &str| s.chars().count();
    if len(&full) <= width {
        return full;
    }
    let tail = format!("~ | {fps}");
    if len(&tail) >= width {
        return fps.chars().take(width).collect();
    }
    let head: String = metrics.chars().take(width - len(&tail)).collect();
    format!("{head}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn key(code: KeyCode, m: KeyModifiers, kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent { code, modifiers: m, kind, state: KeyEventState::NONE })
    }

    #[test]
    fn keys() {
        let p = KeyEventKind::Press;
        assert_eq!(map_event(&key(KeyCode::Char('q'), KeyModifiers::NONE, p)), Action::Quit);
        assert_eq!(map_event(&key(KeyCode::Esc, KeyModifiers::NONE, p)), Action::Quit);
        assert_eq!(map_event(&key(KeyCode::Char('c'), KeyModifiers::CONTROL, p)), Action::Quit);
        assert_eq!(map_event(&key(KeyCode::Char('c'), KeyModifiers::NONE, p)), Action::None);
        assert_eq!(map_event(&key(KeyCode::Char('s'), KeyModifiers::NONE, p)), Action::ToggleStats);
        assert_eq!(
            map_event(&key(KeyCode::Char('s'), KeyModifiers::NONE, KeyEventKind::Release)),
            Action::None
        );
        assert_eq!(
            map_event(&key(KeyCode::Char('s'), KeyModifiers::NONE, KeyEventKind::Repeat)),
            Action::None
        );
        assert_eq!(map_event(&Event::Resize(80, 24)), Action::Resize(80, 24));
        assert_eq!(map_event(&key(KeyCode::Char('l'), KeyModifiers::NONE, p)), Action::ToggleLegend);
        assert_eq!(map_event(&key(KeyCode::Char('W'), KeyModifiers::SHIFT, p)), Action::ToggleWireframe);
        assert_eq!(map_event(&key(KeyCode::Char('t'), KeyModifiers::NONE, p)), Action::ToggleTags);
        assert_eq!(map_event(&key(KeyCode::F(1), KeyModifiers::NONE, p)), Action::ToggleDebug);
        assert_eq!(map_event(&key(KeyCode::F(1), KeyModifiers::NONE, KeyEventKind::Repeat)), Action::None, "held F1 doesn't flicker");
    }

    #[test]
    fn input_watch_wakes_on_input_but_not_jitter() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        use std::time::Duration;
        let mouse = |kind, column, row| Event::Mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE });
        let (early, late) = (Duration::from_millis(200), Duration::from_secs(2));
        let mut w = InputWatch::new();
        assert!(!w.wakes_at(&mouse(MouseEventKind::Moved, 10, 10), early), "settling");
        assert!(!w.wakes_at(&mouse(MouseEventKind::Moved, 40, 20), early), "still settling: new origin");
        assert!(!w.wakes_at(&mouse(MouseEventKind::Moved, 41, 21), late), "1-cell jitter");
        assert!(w.wakes_at(&mouse(MouseEventKind::Moved, 43, 20), late), "real movement");
        let mut w = InputWatch::new();
        assert!(!w.wakes_at(&mouse(MouseEventKind::Down(MouseButton::Left), 1, 1), early));
        assert!(w.wakes_at(&mouse(MouseEventKind::Down(MouseButton::Left), 1, 1), late));
        assert!(w.wakes_at(&mouse(MouseEventKind::ScrollDown, 1, 1), late));
        assert!(w.wakes_at(&key(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Press), early), "keys always");
        assert!(!w.wakes_at(&key(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Release), late));
        assert!(!w.wakes_at(&Event::Resize(80, 24), late));
    }

    #[test]
    fn short_labels() {
        use MetricKind::*;
        assert_eq!(short_label(&rd(Cpu, Some(37.4), None, None, None)), "CPU 37%");
        assert_eq!(short_label(&rd(Net, None, Some(1.0), Some(0.5), None)), "NET 1.5M");
        assert_eq!(short_label(&rd(Nic, None, None, None, Some("Ethernet 2 long"))), "NIC Ethernet 2 n/a");
    }

    fn rd(kind: MetricKind, pct: Option<f32>, a: Option<f64>, b: Option<f64>, name: Option<&str>) -> Reading {
        Reading { kind, name: name.map(String::from), pct, a, b, value: None }
    }

    #[test]
    fn sensor_and_core_labels() {
        use MetricKind::*;
        let v = |kind, value| Reading { value, ..Reading::empty(kind) };
        assert_eq!(segment(&rd(Cpu, Some(37.0), None, None, Some("3"))), "CPU3 37%");
        assert_eq!(segment(&v(CpuTemp, Some(64.4))), "CPU T 64\u{b0}C");
        assert_eq!(segment(&v(CpuFreq, Some(4213.0))), "CPU 4.21 GHz");
        assert_eq!(segment(&v(GpuVolt, Some(0.95))), "GPU Vcore 0.95 V");
        assert_eq!(segment(&v(GpuTemp, None)), "GPU T n/a");
        assert_eq!(short_label(&v(CpuTemp, Some(64.4))), "CPU T 64C");
        assert_eq!(short_label(&v(GpuFreq, Some(1785.0))), "GPU 1.8G");
        assert_eq!(short_label(&v(CpuVolt, Some(1.214))), "CPU 1.21V");
        assert_eq!(short_label(&rd(Cpu, Some(5.0), None, None, Some("11"))), "CPU11 5%");
    }

    #[test]
    fn stats_format() {
        use MetricKind::*;
        let rs = [
            rd(Cpu, Some(37.2), None, None, None),
            rd(Ram, Some(62.0), None, None, None),
            rd(Disk, None, Some(12.44), Some(3.1), None),
            rd(Net, None, Some(1.2), Some(0.3), None),
            rd(Gpu, Some(8.0), None, None, None),
        ];
        let refs: Vec<&Reading> = rs.iter().collect();
        assert_eq!(
            stats_line(&refs, 48.0, 52.0),
            "CPU 37% | RAM 62% | DSK r12.4 w3.1 MB/s | NET ↓1.2 ↑0.3 MB/s | GPU 8% | fps 48/52"
        );
        let na: Vec<Reading> = [Cpu, Ram, Disk, Net, Gpu].into_iter().map(|k| rd(k, None, None, None, None)).collect();
        let refs: Vec<&Reading> = na.iter().collect();
        assert_eq!(stats_line(&refs, 5.0, 5.0), "CPU n/a | RAM n/a | DSK n/a | NET n/a | GPU n/a | fps 5/5");

        let extra = [
            rd(Nic, None, Some(1.0), Some(0.5), Some("Wi-Fi")),
            rd(Nic, None, None, None, None),
            rd(DiskDev, None, Some(0.0), Some(2.31), Some(r"C:\")),
        ];
        let refs: Vec<&Reading> = extra.iter().collect();
        assert_eq!(stats_line(&refs, 9.0, 10.0), r"NIC Wi-Fi ↓1.0 ↑0.5 MB/s | NIC auto n/a | DEV C:\ r0.0 w2.3 MB/s | fps 9/10");
    }

    #[test]
    fn fitted_line_keeps_fps() {
        let rs = [rd(MetricKind::Cpu, Some(37.0), None, None, None), rd(MetricKind::Ram, Some(62.0), None, None, None)];
        let refs: Vec<&Reading> = rs.iter().collect();
        assert_eq!(stats_line_fit(&refs, 48.0, 52.0, "", 80), "CPU 37% | RAM 62% | fps 48/52");
        let cut = stats_line_fit(&refs, 48.0, 52.0, " | q-1", 24);
        assert_eq!(cut, "CPU 3~ | fps 48/52 | q-1");
        assert_eq!(cut.chars().count(), 24);
        assert_eq!(stats_line_fit(&refs, 48.0, 52.0, "", 5), "fps 4");
    }
}
