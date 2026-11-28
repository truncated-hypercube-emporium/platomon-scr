//! The F1 debug panel: the command line, options, frame rate, the quality
//! governor and the renderer, as a box of text stamped over the picture.
//! Both the terminal app and the screensaver fill one in and draw it.

use crate::config::Config;
use crate::render::{AnyRenderer, QualityGovernor, RenderSettings};
use crate::scene::Scene;

/// A smoothed duration or rate (exponential average) and the largest seen
/// since the panel was last read.
#[derive(Debug, Clone, Copy, Default)]
pub struct Avg {
    avg: f32,
    max: f32,
    n: u64,
}

impl Avg {
    pub fn push(&mut self, x: f32) {
        self.avg = if self.n == 0 { x } else { self.avg + (x - self.avg) * 0.1 };
        self.max = self.max.max(x);
        self.n += 1;
    }

    /// Average and peak, in milliseconds for seconds pushed.
    pub fn ms(&self) -> String {
        if self.n == 0 { "-".into() } else { format!("{:.2} ms (peak {:.2})", self.avg * 1e3, self.max * 1e3) }
    }

    /// Forget the peak (a new reporting window).
    pub fn reset_peak(&mut self) {
        self.max = self.avg;
    }
}

/// Rows of the panel: `None` value = a section heading.
#[derive(Debug, Default)]
pub struct DebugPanel {
    rows: Vec<(String, Option<String>)>,
}

impl DebugPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn section(&mut self, title: &str) -> &mut Self {
        self.rows.push((title.to_string(), None));
        self
    }

    pub fn row(&mut self, name: &str, value: impl std::fmt::Display) -> &mut Self {
        self.rows.push((name.to_string(), Some(value.to_string())));
        self
    }

    /// The panel framed as a box exactly `w × h` cells (the left half of
    /// the picture): rows past the box's height are dropped, long values
    /// cut with an ellipsis, the rest of the box left blank.
    pub fn boxed(&self, w: usize, h: usize) -> Vec<String> {
        if w < 8 || h < 3 {
            return Vec::new();
        }
        let inner = w - 4;
        let mut lines = self.lines_inner(inner);
        lines.truncate(h - 2);
        let title = " platomon debug (F1 closes) ";
        let top: String = title.chars().chain(std::iter::repeat('─')).take(w - 2).collect();
        let mut out = vec![format!("┌{top}┐")];
        out.extend(lines.iter().map(|l| format!("│ {l} │")));
        out.resize(h - 1, format!("│{}│", " ".repeat(w - 2)));
        out.push(format!("└{}┘", "─".repeat(w - 2)));
        out
    }

    /// The rows as `inner`-wide text.
    fn lines_inner(&self, inner: usize) -> Vec<String> {
        let name_w = self.rows.iter().filter(|r| r.1.is_some()).map(|r| r.0.chars().count()).max().unwrap_or(0);
        self.rows
            .iter()
            .map(|(n, v)| {
                let l = match v {
                    Some(v) => format!("{n:<name_w$}  {v}"),
                    None => format!("[{n}]"),
                };
                let mut s: String = l.chars().take(inner).collect();
                if l.chars().count() > inner {
                    s.pop();
                    s.push('…');
                }
                let pad = inner - s.chars().count();
                s + &" ".repeat(pad)
            })
            .collect()
    }

    /// The panel's text lines, framed, at most `max_w` wide (long values
    /// are cut with an ellipsis).
    pub fn lines(&self, max_w: usize) -> Vec<String> {
        let name_w = self.rows.iter().filter(|r| r.1.is_some()).map(|r| r.0.chars().count()).max().unwrap_or(0);
        let body: Vec<String> = self
            .rows
            .iter()
            .map(|(n, v)| match v {
                Some(v) => format!("{n:<name_w$}  {v}"),
                None => format!("[{n}]"),
            })
            .collect();
        let inner = body.iter().map(|l| l.chars().count()).max().unwrap_or(0).min(max_w.saturating_sub(4));
        if max_w < 8 || inner == 0 {
            return Vec::new();
        }
        let title = " platomon debug (F1 closes) ";
        let top_fill = (inner + 2).saturating_sub(title.chars().count());
        let mut out = vec![format!("┌{}{}┐", title.chars().take(inner + 2).collect::<String>(), "─".repeat(top_fill))];
        for l in &body {
            let mut s: String = l.chars().take(inner).collect();
            if l.chars().count() > inner {
                s.pop();
                s.push('…');
            }
            let pad = inner - s.chars().count();
            out.push(format!("│ {s}{} │", " ".repeat(pad)));
        }
        out.push(format!("└{}┘", "─".repeat(inner + 2)));
        out
    }
}

/// Stamp `lines` into a `w × h` cell grid at its top-left corner, clipped
/// to it. `colors` (if any) are set to `ink` under the panel.
pub fn stamp(lines: &[String], chars: &mut [char], colors: Option<&mut [u32]>, w: usize, h: usize, ink: u32) {
    let mut colors = colors;
    for (k, line) in lines.iter().enumerate() {
        let row = k;
        if row >= h {
            break;
        }
        for (j, c) in line.chars().enumerate() {
            let col = j;
            if col >= w {
                break;
            }
            let i = row * w + col;
            if let Some(slot) = chars.get_mut(i) {
                *slot = c;
            }
            if let Some(cs) = colors.as_deref_mut()
                && let Some(slot) = cs.get_mut(i)
            {
                *slot = ink;
            }
        }
    }
}

/// The program line and build: the command line, version and features.
pub fn program_rows(p: &mut DebugPanel, started: std::time::Instant, frames: u64) {
    let args: Vec<String> = std::env::args().collect();
    let mut features = Vec::new();
    if cfg!(feature = "gpu") {
        features.push("gpu");
    }
    if cfg!(feature = "nvidia") {
        features.push("nvidia");
    }
    if cfg!(feature = "screensaver") {
        features.push("screensaver");
    }
    p.section("program")
        .row("command", args.join(" "))
        .row("version", format!("{} ({})", env!("CARGO_PKG_VERSION"), if features.is_empty() { "no features".into() } else { features.join(", ") }))
        .row("uptime", format!("{:.1} s, {frames} full frames", started.elapsed().as_secs_f32()))
        .row("render threads", rayon::current_num_threads());
}

/// The renderer and the quality governor: what this frame used.
pub fn renderer_rows(p: &mut DebugPanel, cfg: &Config, r: &AnyRenderer, gov: &QualityGovernor, base: &RenderSettings, used: &RenderSettings) {
    let device = match r.gpu_name() {
        Some(g) if r.on_gpu() => format!("GPU: {g} (Vulkan)"),
        Some(g) => format!("CPU (GPU {g} idle)"),
        None => format!("CPU, {} threads", rayon::current_num_threads()),
    };
    p.section("renderer").row("device", device);
    if let Some(why) = r.gpu_problem() {
        p.row("space heater", format!("off: {why}"));
    } else if cfg.gpu {
        let pixels = match (cfg.gpu_pixels, r.present_problem()) {
            (false, _) => "on".to_string(),
            (true, Some(why)) => format!("on, discrete pixels read back to the CPU ({why})"),
            (true, None) => "on, discrete pixels straight to the screen".to_string(),
        };
        p.row("space heater", pixels);
    }
    p.row(
        "governor",
        if cfg.adaptive_quality {
            format!("level {}/{} at {:.0}% of the frame budget", gov.level(), QualityGovernor::MAX_LEVEL, gov.load() * 100.0)
        } else {
            "off (adaptive quality off)".into()
        },
    )
    .row("samples/bounces", format!("{} / {} (set {} / {})", used.spp, used.trace.bounces, base.spp, base.trace.bounces))
    .row("march", format!("{} steps, {} distance{}", used.trace.max_steps, used.trace.max_dist, if cfg.far_ground { ", far floor" } else { "" }));
}

/// The scene and the look: the options that shape the picture.
pub fn scene_rows(p: &mut DebugPanel, cfg: &Config, scene: &Scene) {
    let kinds: Vec<&str> = scene.solids.iter().map(|s| s.poly.kind().id()).collect();
    p.section("scene")
        .row("solids", format!("{}: {}", kinds.len(), kinds.join(", ")))
        .row("metrics", cfg.show.join(", "))
        .row("layout / camera", format!("{:?} / {:?}", cfg.layout, cfg.camera))
        .row(
            "floor",
            format!(
                "{} lum {} contrast {:.2} tile {:.2} turn {:?} scroll {:?} sway {:.2}",
                cfg.bg_pattern.id(),
                cfg.bg_lum,
                cfg.bg_contrast,
                cfg.bg_scale,
                cfg.bg_rotate,
                cfg.bg_scroll,
                cfg.bg_sway
            ),
        )
        .row("colour", format!("{} by {:?}, background {}", cfg.color, cfg.color_by, cfg.bg_color))
        .row("text", format!("ramp {} glyphs, exposure {:.2}, cell aspect {:.3}", crate::ascii::Ramp::new(&cfg.ramp).len(), cfg.exposure, cfg.char_aspect))
        .row("seed", cfg.seed.map_or_else(|| "random".to_string(), |s| s.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_is_framed_and_clipped() {
        let mut p = DebugPanel::new();
        p.section("frame").row("fps", "59.9 of 60").row("a very long name", "x".repeat(200));
        let lines = p.lines(40);
        assert!(lines.len() == 5 && lines.iter().all(|l| l.chars().count() == lines[0].chars().count()), "{lines:#?}");
        assert!(lines[0].starts_with('┌') && lines[4].starts_with('└'));
        assert!(lines.iter().all(|l| l.chars().count() <= 40));
        assert!(lines[3].contains('…'), "long values are cut");
        assert!(lines[2].contains("fps") && lines[2].contains("59.9"));
        // the left half: a box exactly w × h, blank below the rows
        let (w, h) = (40, 12);
        let half = p.boxed(w / 2, h);
        assert_eq!(half.len(), h);
        assert!(half.iter().all(|l| l.chars().count() == w / 2), "{half:#?}");
        assert!(half[0].starts_with("┌ platomon debug") && half[h - 1].starts_with('└'));
        assert!(half[6].trim_matches(['│', ' ']).is_empty(), "blank under the rows");
        assert_eq!(p.boxed(w / 2, 4).len(), 4, "rows dropped to fit");
        // stamped at the top-left, clipped to the grid, ink under it
        let mut chars = vec!['.'; w * h];
        let mut cols = vec![0u32; w * h];
        stamp(&half, &mut chars, Some(&mut cols), w, h, 7);
        assert_eq!((chars[0], chars[w / 2 - 1], chars[w / 2]), ('┌', '┐', '.'), "the left half only");
        assert_eq!((cols[0], cols[w / 2]), (7, 0));
        assert!(chars[(h - 1) * w..(h - 1) * w + w / 2].iter().all(|&c| c != '.'), "down to the last row");
        assert!(DebugPanel::new().lines(40).is_empty());
        let mut a = Avg::default();
        a.push(0.010);
        a.push(0.030);
        assert!(a.ms().starts_with("12.00 ms (peak 30.00)"), "{}", a.ms());
    }
}
