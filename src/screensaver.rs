//! Screensaver planning: a random but cheap configuration (with the user's
//! fixed choices from the configuration dialog on top), a text size chosen so
//! rendering at the display's refresh rate stays within a CPU budget on this
//! machine, and the OLED burn-in pixel shift.

use std::time::Instant;

use crate::config::{Config, SlotSpec};
use crate::metrics::Channel;
use crate::render::{RenderSettings, Renderer, TraceSettings};
use crate::scene::camera::{CameraMode, CameraParams, OrbitDir};
use crate::scene::ground::{Pattern, Spin};
use crate::scene::sdf::SolidKind;
use crate::scene::{AnimParams, Layout, Scene, SlotLayout};
use crate::color::{self, ColorBy};

/// The primary display as Windows reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Display {
    pub width: u32,
    pub height: u32,
    /// Hz; 0 or 1 = unknown ("hardware default").
    pub refresh: u32,
}

/// Share of wall time spent rendering at the full frame rate (all cores).
pub const RENDER_BUDGET: f32 = 0.15;
/// Console font height range, pixels.
pub const FONT_MIN: i16 = 8;
pub const FONT_MAX: i16 = 40;
/// Width / height of a Consolas cell (only used to plan; the real value is
/// measured once the font is set).
pub const EST_CHAR_ASPECT: f32 = 0.55;
/// Frame-rate cap range when following the refresh rate.
pub const FPS_CAP: (f32, f32) = (30.0, 240.0);

fn range(rng: &mut fastrand::Rng, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * rng.f32()
}

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

/// Frame-rate ceiling for a display: its refresh rate, clamped.
pub fn fps_for(d: &Display) -> f32 {
    if d.refresh <= 1 { 60.0 } else { (d.refresh as f32).clamp(FPS_CAP.0, FPS_CAP.1) }
}

/// A random configuration. Only cheap rendering settings are used: 1 sample
/// per pixel, 1 bounce, adaptive quality on.
pub fn randomize(rng: &mut fastrand::Rng, d: &Display) -> Config {
    let mut c = Config {
        spp: 1,
        bounces: 1,
        adaptive_quality: true,
        fps_max: fps_for(d),
        seed: Some(rng.u64(..)),
        ..Config::default()
    };

    // metrics and solids
    let mut show: Vec<String> = ["cpu", "ram", "disk", "net", "gpu"].map(String::from).to_vec();
    for extra in ["nic", "disk-dev"] {
        if rng.bool() {
            show.push(extra.to_string());
        }
    }
    let mut kinds = SolidKind::ALL.to_vec();
    rng.shuffle(&mut kinds);
    for (m, k) in show.iter().zip(&kinds) {
        c.solids.insert(m.clone(), k.id().to_string());
    }
    c.show = show;
    c.layout = if rng.bool() { Layout::Rings } else { Layout::Sphere };

    // camera
    c.camera = match rng.u8(0..20) {
        0..=4 => CameraMode::Static,
        5..=13 => CameraMode::Orbit,
        _ => CameraMode::Focus,
    };
    c.orbit_period = range(rng, 40.0, 120.0).round();
    c.orbit_dir = OrbitDir::Random;
    c.orbit_bob = round2(range(rng, 0.0, 0.6));
    c.focus_zoom = round2(range(rng, 0.3, 0.85));
    c.focus_tau = round2(range(rng, 1.0, 3.0));

    // background
    c.bg_lum = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 4.0, 5.0, 6.0][rng.usize(0..9)];
    c.bg_pattern = Pattern::ALL[rng.usize(0..Pattern::ALL.len())];
    c.bg_contrast = round2(range(rng, 0.3, 0.9));
    c.bg_scale = round2(range(rng, 0.7, 2.5));
    if rng.bool() {
        c.bg_scroll = [round2(range(rng, -0.4, 0.4)), round2(range(rng, -0.4, 0.4))];
    }
    if rng.u8(0..10) < 4 {
        c.bg_sway = round2(range(rng, 0.2, 1.0));
        c.bg_sway_period = range(rng, 6.0, 20.0).round();
    }
    c.bg_rotate = [Spin::Off, Spin::Cw, Spin::Ccw][rng.usize(0..3)];
    c.bg_rotate_period = range(rng, 25.0, 90.0).round();

    // colour
    if rng.u8(0..10) >= 3 {
        let presets: Vec<&str> = color::PRESETS.iter().map(|p| p.0).filter(|id| *id != "grey").collect();
        c.color = presets[rng.usize(0..presets.len())].to_string();
        c.color_by = if rng.u8(0..10) < 4 { ColorBy::Load } else { ColorBy::Lum };
        c.color_reverse = rng.u8(0..20) < 3;
    }

    // overlays and the rest
    c.wireframe = rng.u8(0..20) < 7;
    // No corner legend in the screensaver.
    c.legend = false;
    c.fps_invert = rng.u8(0..4) == 0;
    c.show_stats = rng.u8(0..5) == 0;
    if rng.u8(0..5) == 0 {
        c.ramp = crate::ascii::CLASSIC_RAMP.to_string();
    }
    c.sanitize();
    c
}

pub fn anim_params(cfg: &Config) -> AnimParams {
    AnimParams { min_scale: cfg.min_scale, max_scale: cfg.max_scale, idle_spin: cfg.idle_spin, max_spin: cfg.max_spin }
}

pub fn camera_params(cfg: &Config) -> CameraParams {
    CameraParams {
        mode: cfg.camera,
        period: cfg.orbit_period,
        dir: cfg.orbit_dir,
        radius: cfg.orbit_radius,
        height: cfg.orbit_height,
        bob: cfg.orbit_bob,
        seed: cfg.seed,
        focus_zoom: cfg.focus_zoom,
        focus_tau: cfg.focus_tau,
    }
}

/// The scene for `cfg`'s resolved slots, with its layout and background.
pub fn build_scene(cfg: &Config, slots: &[SlotSpec]) -> Scene {
    let layouts: Vec<SlotLayout> = slots.iter().map(|s| SlotLayout { solid: s.solid, ring: s.ring }).collect();
    Scene::with_layout(&layouts, &anim_params(cfg), &camera_params(cfg), cfg.layout).with_ground(&cfg.ground_params())
}

/// Render settings for `cfg` (no picture-ratio framing: the screen is filled).
pub fn render_settings(cfg: &Config, char_aspect: f32) -> RenderSettings {
    RenderSettings {
        spp: cfg.spp,
        trace: TraceSettings { bounces: cfg.bounces, max_steps: cfg.max_steps, max_dist: cfg.max_dist },
        char_aspect,
        fit_aspect: None,
    }
}

/// Render time per cell for `cfg`'s scene at full load, seconds (all cores,
/// median of several frames).
pub fn seconds_per_cell(cfg: &Config) -> Result<f32, String> {
    let slots = cfg.resolve_slots()?;
    let anim = anim_params(cfg);
    let mut scene = build_scene(cfg, &slots);
    // Worst case: every solid at full size.
    let ch = vec![Channel { available: true, level: 1.0, spin_y: 1.0, spin_x: 0.3, activity: 1.0 }; slots.len()];
    for _ in 0..5 {
        scene.update(&ch, &anim, 0.1);
    }
    let rs = render_settings(cfg, EST_CHAR_ASPECT);
    let (w, h) = (160, 50);
    let mut r = Renderer::new();
    for _ in 0..2 {
        r.render(&scene, w, h, &rs);
    }
    let mut times: Vec<f32> = (0..7)
        .map(|_| {
            let t = Instant::now();
            r.render(&scene, w, h, &rs);
            t.elapsed().as_secs_f32()
        })
        .collect();
    times.sort_by(f32::total_cmp);
    Ok((times[times.len() / 2] / (w * h) as f32).max(1e-9))
}

/// Font height (pixels) so a full-screen grid rendered at `fps` keeps
/// rendering within `budget` (share of wall time on all cores, e.g.
/// [`RENDER_BUDGET`]). Returns (height, est. cols, est. rows).
pub fn font_height(d: &Display, fps: f32, sec_per_cell: f32, budget: f32) -> (i16, u32, u32) {
    let (w, h) = (d.width.max(320) as f32, d.height.max(200) as f32);
    let cells = budget.clamp(0.01, 1.0) / (fps.max(1.0) * sec_per_cell.max(1e-9));
    // cols = rows · (w/h) / aspect  ⇒  rows = √(cells · aspect · h / w)
    let rows = (cells * EST_CHAR_ASPECT * h / w).sqrt().max(1.0);
    let px = (h / rows).ceil().clamp(FONT_MIN as f32, FONT_MAX as f32) as i16;
    let rows = (h / px as f32).floor() as u32;
    let cols = (w / (px as f32 * EST_CHAR_ASPECT)).floor() as u32;
    (px, cols, rows)
}

/// Everything the launcher needs for one run.
#[derive(Debug, Clone)]
pub struct Plan {
    pub config: Config,
    pub font_px: i16,
    pub est_grid: (u32, u32),
    pub sec_per_cell: f32,
    pub budget: f32,
    pub settings: ScrSettings,
}

/// Random look, then the user's fixed choices from `s` on top; frame-rate
/// cap and CPU budget from `s`; text size from a speed test.
pub fn plan(d: &Display, seed: u64, s: &ScrSettings) -> Result<Plan, String> {
    let mut rng = fastrand::Rng::with_seed(seed);
    let mut config = randomize(&mut rng, d);
    s.apply(&mut config);
    config.fps_max = match s.fps_cap {
        Some(f) => (f as f32).clamp(FPS_CAP.0, FPS_CAP.1),
        None => fps_for(d),
    };
    config.sanitize();
    let spc = seconds_per_cell(&config)?;
    let budget = s.budget_pct as f32 / 100.0;
    let (font_px, cols, rows) = font_height(d, config.fps_max, spc, budget);
    Ok(Plan { config, font_px, est_grid: (cols, rows), sec_per_cell: spc, budget, settings: s.clone() })
}

impl Plan {
    /// The config as TOML, with a header recording how it was chosen.
    pub fn to_toml(&self, d: &Display, seed: u64) -> String {
        format!(
            "# platomon screensaver run (seed {seed})\n\
             # display {}x{} @ {} Hz -> fps_max {}\n\
             # render {:.3} us/cell -> Consolas {} px, about {}x{} cells, budget {:.0}% of wall time\n\n{}",
            d.width,
            d.height,
            d.refresh,
            self.config.fps_max,
            self.sec_per_cell * 1e6,
            self.font_px,
            self.est_grid.0,
            self.est_grid.1,
            self.budget * 100.0,
            self.config.to_toml()
        )
    }
}

// ------------------------------------------------------------------ settings

/// Which optional metrics to add to the five main ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extras {
    None,
    Nic,
    Disk,
    Both,
}

/// The user's choices from the configuration dialog, stored in the
/// screensaver's INI file. `None` = pick at random each run.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrSettings {
    pub layout: Option<Layout>,
    pub camera: Option<CameraMode>,
    /// `"off"` or a gradient preset id.
    pub color: Option<String>,
    pub color_by: Option<ColorBy>,
    pub bg_pattern: Option<Pattern>,
    pub bg_lum: Option<u8>,
    pub wireframe: Option<bool>,
    pub stats: Option<bool>,
    pub fps_invert: Option<bool>,
    pub extras: Option<Extras>,
    /// Frame-rate cap; `None` = the display's refresh rate.
    pub fps_cap: Option<u32>,
    /// Rendering CPU budget, percent of wall time at full frame rate.
    pub budget_pct: u8,
    /// OLED burn-in protection: move the picture 1 px at random intervals.
    pub shift: bool,
    pub shift_min_frames: u32,
    pub shift_max_frames: u32,
    /// Largest drift from the centred position, pixels per axis.
    pub shift_radius: u8,
}

impl Default for ScrSettings {
    fn default() -> Self {
        Self {
            layout: None,
            camera: None,
            color: None,
            color_by: None,
            bg_pattern: None,
            bg_lum: None,
            wireframe: None,
            stats: None,
            fps_invert: None,
            extras: None,
            fps_cap: None,
            budget_pct: (RENDER_BUDGET * 100.0).round() as u8,
            shift: true,
            shift_min_frames: 600,
            shift_max_frames: 3600,
            shift_radius: 2,
        }
    }
}

/// INI section name (`szAppName` in the Win32 screensaver sample).
pub const INI_SECTION: &str = "platomon";
pub const RANDOM: &str = "random";
pub const SHIFT_FRAMES_MAX: u32 = 1_000_000;

fn on_off(v: Option<bool>) -> String {
    match v {
        None => RANDOM.into(),
        Some(true) => "on".into(),
        Some(false) => "off".into(),
    }
}

fn parse_on_off(s: &str) -> Option<Option<bool>> {
    match s {
        RANDOM => Some(None),
        "on" => Some(Some(true)),
        "off" => Some(Some(false)),
        _ => None,
    }
}

fn opt_id<T>(v: &Option<T>, id: impl Fn(&T) -> String) -> String {
    v.as_ref().map_or_else(|| RANDOM.to_string(), id)
}

fn layout_id(l: &Layout) -> String {
    match l {
        Layout::Rings => "rings",
        Layout::Sphere => "sphere",
    }
    .into()
}

fn camera_id(c: &CameraMode) -> String {
    match c {
        CameraMode::Static => "static",
        CameraMode::Orbit => "orbit",
        CameraMode::Focus => "focus",
    }
    .into()
}

fn color_by_id(c: &ColorBy) -> String {
    match c {
        ColorBy::Lum => "lum",
        ColorBy::Load => "load",
    }
    .into()
}

fn extras_id(e: &Extras) -> String {
    match e {
        Extras::None => "none",
        Extras::Nic => "nic",
        Extras::Disk => "disk",
        Extras::Both => "both",
    }
    .into()
}

/// A dialog drop-down: INI key, label, and (INI value, shown text) choices.
pub struct Choice {
    pub key: &'static str,
    pub label: &'static str,
    pub options: Vec<(String, String)>,
}

/// Every drop-down in the configuration dialog, in display order.
pub fn choices() -> Vec<Choice> {
    let rnd = || (RANDOM.to_string(), "Random".to_string());
    let opt = |v: &str, t: &str| (v.to_string(), t.to_string());
    let tri = || vec![rnd(), opt("on", "On"), opt("off", "Off")];
    let mut colors = vec![rnd(), opt("off", "Off (monochrome)")];
    colors.extend(color::PRESETS.iter().map(|(id, _, _)| opt(id, id)));
    let mut patterns = vec![rnd()];
    patterns.extend(Pattern::ALL.iter().map(|p| opt(p.id(), p.id())));
    let mut lums = vec![rnd()];
    lums.extend((0..=10).map(|l| opt(&l.to_string(), &if l == 0 { "0 (no floor)".into() } else { l.to_string() })));
    let mut caps = vec![opt("display", "Display refresh rate")];
    caps.extend([30, 60, 75, 90, 120, 144, 165, 240].map(|f| opt(&f.to_string(), &format!("{f} fps"))));
    let budgets = [5, 10, 15, 20, 25, 30, 40, 50].map(|b| opt(&b.to_string(), &format!("{b}%"))).to_vec();
    vec![
        Choice { key: "layout", label: "Layout", options: vec![rnd(), opt("rings", "Rings"), opt("sphere", "Sphere")] },
        Choice {
            key: "camera",
            label: "Camera",
            options: vec![rnd(), opt("static", "Static"), opt("orbit", "Orbit"), opt("focus", "Focus on busiest")],
        },
        Choice { key: "color", label: "Colour", options: colors },
        Choice {
            key: "color_by",
            label: "Colour source",
            options: vec![rnd(), opt("lum", "Brightness"), opt("load", "Load per solid")],
        },
        Choice { key: "bg_pattern", label: "Background pattern", options: patterns },
        Choice { key: "bg_lum", label: "Background luminosity", options: lums },
        Choice { key: "wireframe", label: "Wireframe overlay", options: tri() },
        Choice { key: "stats", label: "Stats bar", options: tri() },
        Choice { key: "fps_invert", label: "Inverted frame rate", options: tri() },
        Choice {
            key: "extras",
            label: "Extra metrics",
            options: vec![
                rnd(),
                opt("none", "None"),
                opt("nic", "Network adapter"),
                opt("disk", "Disk device"),
                opt("both", "Both"),
            ],
        },
        Choice { key: "fps_cap", label: "Frame-rate cap", options: caps },
        Choice { key: "budget", label: "CPU budget", options: budgets },
        Choice {
            key: "shift_radius",
            label: "Burn-in shift range",
            options: (1..=4).map(|r| opt(&r.to_string(), &format!("\u{b1}{r} px"))).collect(),
        },
    ]
}

impl ScrSettings {
    /// `(key, value)` pairs as written to the INI file.
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("layout", opt_id(&self.layout, layout_id)),
            ("camera", opt_id(&self.camera, camera_id)),
            ("color", opt_id(&self.color, |c| c.clone())),
            ("color_by", opt_id(&self.color_by, color_by_id)),
            ("bg_pattern", opt_id(&self.bg_pattern, |p| p.id().to_string())),
            ("bg_lum", opt_id(&self.bg_lum, |l| l.to_string())),
            ("wireframe", on_off(self.wireframe)),
            ("stats", on_off(self.stats)),
            ("fps_invert", on_off(self.fps_invert)),
            ("extras", opt_id(&self.extras, extras_id)),
            ("fps_cap", self.fps_cap.map_or_else(|| "display".to_string(), |f| f.to_string())),
            ("budget", self.budget_pct.to_string()),
            ("shift", if self.shift { "on" } else { "off" }.to_string()),
            ("shift_min_frames", self.shift_min_frames.to_string()),
            ("shift_max_frames", self.shift_max_frames.to_string()),
            ("shift_radius", self.shift_radius.to_string()),
        ]
    }

    /// Read settings through `get(key)`; missing or invalid values fall back
    /// to the defaults.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let val = |k: &str| get(k).map(|v| v.trim().to_ascii_lowercase()).filter(|v| !v.is_empty());
        let enum_opt = |k: &str, parse: &dyn Fn(&str) -> bool| -> Option<Option<String>> {
            let v = val(k)?;
            if v == RANDOM {
                Some(None)
            } else if parse(&v) {
                Some(Some(v))
            } else {
                None
            }
        };
        let preset_ok = |v: &str| v == "off" || color::PRESETS.iter().any(|(id, _, _)| *id == v);
        let pattern_of = |v: &str| Pattern::ALL.into_iter().find(|p| p.id() == v);
        let num = |k: &str| val(k).and_then(|v| v.parse::<u32>().ok());
        let shift_min = num("shift_min_frames").filter(|v| (1..=SHIFT_FRAMES_MAX).contains(v)).unwrap_or(d.shift_min_frames);
        let shift_max = num("shift_max_frames")
            .filter(|v| (1..=SHIFT_FRAMES_MAX).contains(v))
            .unwrap_or(d.shift_max_frames)
            .max(shift_min);
        Self {
            layout: enum_opt("layout", &|v| v.parse::<Layout>().is_ok())
                .map_or(d.layout, |o| o.and_then(|v| v.parse().ok())),
            camera: enum_opt("camera", &|v| v.parse::<CameraMode>().is_ok())
                .map_or(d.camera, |o| o.and_then(|v| v.parse().ok())),
            color: enum_opt("color", &preset_ok).unwrap_or(d.color.clone()),
            color_by: enum_opt("color_by", &|v| v.parse::<ColorBy>().is_ok())
                .map_or(d.color_by, |o| o.and_then(|v| v.parse().ok())),
            bg_pattern: enum_opt("bg_pattern", &|v| pattern_of(v).is_some())
                .map_or(d.bg_pattern, |o| o.and_then(|v| pattern_of(&v))),
            bg_lum: enum_opt("bg_lum", &|v| v.parse::<u8>().is_ok_and(|l| l <= 10))
                .map_or(d.bg_lum, |o| o.and_then(|v| v.parse().ok())),
            wireframe: val("wireframe").and_then(|v| parse_on_off(&v)).unwrap_or(d.wireframe),
            stats: val("stats").and_then(|v| parse_on_off(&v)).unwrap_or(d.stats),
            fps_invert: val("fps_invert").and_then(|v| parse_on_off(&v)).unwrap_or(d.fps_invert),
            extras: enum_opt("extras", &|v| matches!(v, "none" | "nic" | "disk" | "both")).map_or(d.extras, |o| {
                o.map(|v| match v.as_str() {
                    "none" => Extras::None,
                    "nic" => Extras::Nic,
                    "disk" => Extras::Disk,
                    _ => Extras::Both,
                })
            }),
            fps_cap: match val("fps_cap").as_deref() {
                None | Some("display") => None,
                Some(v) => v.parse::<u32>().ok().filter(|f| (30..=240).contains(f)).or(d.fps_cap),
            },
            budget_pct: num("budget").filter(|b| (1..=100).contains(b)).map_or(d.budget_pct, |b| b as u8),
            shift: val("shift").and_then(|v| parse_on_off(&v)).flatten().unwrap_or(d.shift),
            shift_min_frames: shift_min,
            shift_max_frames: shift_max,
            shift_radius: num("shift_radius").filter(|r| (1..=8).contains(r)).map_or(d.shift_radius, |r| r as u8),
        }
    }

    /// Put the fixed choices on top of a random configuration.
    pub fn apply(&self, c: &mut Config) {
        if let Some(l) = self.layout {
            c.layout = l;
        }
        if let Some(m) = self.camera {
            c.camera = m;
        }
        if let Some(col) = &self.color {
            c.color = col.clone();
        }
        if let Some(by) = self.color_by {
            c.color_by = by;
            // A fixed source with a random "off" still means: use colour.
            if self.color.is_none() && c.color == "off" {
                c.color = color::default_for(by).to_string();
            }
        }
        if let Some(p) = self.bg_pattern {
            c.bg_pattern = p;
        }
        if let Some(l) = self.bg_lum {
            c.bg_lum = l as f32;
        }
        if let Some(w) = self.wireframe {
            c.wireframe = w;
        }
        if let Some(s) = self.stats {
            c.show_stats = s;
        }
        if let Some(i) = self.fps_invert {
            c.fps_invert = i;
        }
        if let Some(e) = self.extras {
            let mut show: Vec<String> = ["cpu", "ram", "disk", "net", "gpu"].map(String::from).to_vec();
            if matches!(e, Extras::Nic | Extras::Both) {
                show.push("nic".into());
            }
            if matches!(e, Extras::Disk | Extras::Both) {
                show.push("disk-dev".into());
            }
            // Keep the random solids; give new metrics unused ones.
            c.solids.retain(|k, _| show.contains(k));
            for m in &show {
                if !c.solids.contains_key(m) {
                    let used: Vec<String> = c.solids.values().cloned().collect();
                    if let Some(k) = SolidKind::ALL.iter().find(|k| !used.iter().any(|u| u == k.id())) {
                        c.solids.insert(m.clone(), k.id().to_string());
                    }
                }
            }
            c.show = show;
        }
    }
}

// ------------------------------------------------------------------ burn-in shift

/// OLED burn-in protection: every N frames (N random in `min..=max`) the
/// picture moves 1 px horizontally and 1 px vertically, drifting within
/// ±`radius` px of its centred position.
#[derive(Debug, Clone)]
pub struct PixelShift {
    offset: (i32, i32),
    radius: i32,
    min: u32,
    max: u32,
    left: u32,
    rng: fastrand::Rng,
}

impl PixelShift {
    pub fn new(seed: u64, radius: u8, min_frames: u32, max_frames: u32) -> Self {
        let min = min_frames.max(1);
        let max = max_frames.max(min);
        let mut rng = fastrand::Rng::with_seed(seed);
        let left = rng.u32(min..=max);
        Self { offset: (0, 0), radius: radius.max(1) as i32, min, max, left, rng }
    }

    pub fn offset(&self) -> (i32, i32) {
        self.offset
    }

    /// Call once per drawn frame; returns true when the picture moved.
    pub fn tick(&mut self) -> bool {
        self.left = self.left.saturating_sub(1);
        if self.left > 0 {
            return false;
        }
        let r = self.radius;
        let step = |v: i32, rng: &mut fastrand::Rng| {
            let d = if rng.bool() { 1 } else { -1 };
            if (v + d).abs() > r { v - d } else { v + d }
        };
        self.offset = (step(self.offset.0, &mut self.rng), step(self.offset.1, &mut self.rng));
        self.left = self.rng.u32(self.min..=self.max);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WQHD: Display = Display { width: 2560, height: 1440, refresh: 144 };

    #[test]
    fn random_configs_are_valid_and_cheap() {
        let mut seen_layouts = std::collections::HashSet::new();
        for seed in 0..200 {
            let c = randomize(&mut fastrand::Rng::with_seed(seed), &WQHD);
            let slots = c.resolve_slots().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            assert!((5..=7).contains(&slots.len()));
            assert_eq!((c.spp, c.bounces, c.adaptive_quality), (1, 1, true), "cheap settings");
            assert!(!c.legend, "no corner legend");
            assert_eq!(c.fps_max, 144.0);
            c.color_map().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            let kinds: std::collections::HashSet<_> = slots.iter().map(|s| s.solid).collect();
            assert_eq!(kinds.len(), slots.len(), "distinct solids");
            // round-trips through the TOML the launcher writes
            assert_eq!(Config::from_toml(&c.to_toml()).unwrap(), c);
            seen_layouts.insert(format!("{:?}{:?}", c.layout, c.camera));
        }
        assert!(seen_layouts.len() >= 5, "varied: {seen_layouts:?}");
        // same seed, same result
        let a = randomize(&mut fastrand::Rng::with_seed(9), &WQHD);
        assert_eq!(a, randomize(&mut fastrand::Rng::with_seed(9), &WQHD));
    }

    #[test]
    fn fps_follows_refresh_rate() {
        assert_eq!(fps_for(&Display { refresh: 60, ..WQHD }), 60.0);
        assert_eq!(fps_for(&Display { refresh: 165, ..WQHD }), 165.0);
        assert_eq!(fps_for(&Display { refresh: 1, ..WQHD }), 60.0, "unknown");
        assert_eq!(fps_for(&Display { refresh: 24, ..WQHD }), 30.0);
        assert_eq!(fps_for(&Display { refresh: 500, ..WQHD }), 240.0);
    }

    #[test]
    fn font_keeps_render_within_budget() {
        let spc = 2e-7; // 0.2 µs per cell
        let (px, cols, rows) = font_height(&WQHD, 144.0, spc, RENDER_BUDGET);
        assert!((FONT_MIN..=FONT_MAX).contains(&px));
        let load = (cols * rows) as f32 * spc * 144.0;
        assert!(load <= RENDER_BUDGET * 1.1, "{px}px {cols}x{rows}: {load}");
        // slower machine or faster display ⇒ bigger font, fewer cells
        let (px_slow, ..) = font_height(&WQHD, 144.0, spc * 4.0, RENDER_BUDGET);
        let (px_60, ..) = font_height(&Display { refresh: 60, ..WQHD }, 60.0, spc, RENDER_BUDGET);
        assert!(px_slow > px && px_60 < px);
        // never smaller than the minimum, never larger than the maximum
        assert_eq!(font_height(&WQHD, 60.0, 1e-12, RENDER_BUDGET).0, FONT_MIN);
        assert_eq!(font_height(&WQHD, 240.0, 1e-3, RENDER_BUDGET).0, FONT_MAX);
    }

    #[test]
    fn plan_measures_and_records() {
        let p = plan(&WQHD, 3, &ScrSettings::default()).unwrap();
        assert!(p.sec_per_cell > 0.0 && p.sec_per_cell < 1e-3);
        let t = p.to_toml(&WQHD, 3);
        assert!(t.starts_with("# platomon screensaver run (seed 3)"));
        assert_eq!(Config::from_toml(&t).unwrap(), p.config, "header is only comments");
    }

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: std::collections::HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn settings_roundtrip_and_defaults() {
        let d = ScrSettings::default();
        assert_eq!(ScrSettings::from_lookup(|_| None), d, "missing file = defaults");
        assert_eq!((d.shift, d.budget_pct, d.fps_cap), (true, 15, None));
        let s = ScrSettings {
            layout: Some(Layout::Sphere),
            camera: Some(CameraMode::Focus),
            color: Some("viridis".into()),
            color_by: Some(ColorBy::Load),
            bg_pattern: Some(Pattern::Hex),
            bg_lum: Some(0),
            wireframe: Some(true),
            stats: Some(false),
            fps_invert: None,
            extras: Some(Extras::Both),
            fps_cap: Some(60),
            budget_pct: 25,
            shift: false,
            shift_min_frames: 10,
            shift_max_frames: 20,
            shift_radius: 3,
        };
        let pairs = s.to_pairs();
        let back = ScrSettings::from_lookup(|k| pairs.iter().find(|(pk, _)| *pk == k).map(|(_, v)| v.clone()));
        assert_eq!(back, s);
        // junk falls back per key; max < min is raised to min
        let j = ScrSettings::from_lookup(lookup(&[
            ("layout", "cube"),
            ("color", "plaid"),
            ("bg_lum", "11"),
            ("fps_cap", "5"),
            ("budget", "0"),
            ("shift_min_frames", "500"),
            ("shift_max_frames", "100"),
            ("wireframe", "ON"),
        ]));
        assert_eq!((j.layout, j.color, j.bg_lum, j.fps_cap, j.budget_pct), (None, None, None, None, 15));
        assert_eq!((j.shift_min_frames, j.shift_max_frames, j.wireframe), (500, 500, Some(true)));
    }

    #[test]
    fn every_dialog_choice_parses() {
        let keys: Vec<&str> = ScrSettings::default().to_pairs().iter().map(|(k, _)| *k).collect();
        for c in choices() {
            assert!(keys.contains(&c.key), "{} is a settings key", c.key);
            for (v, _) in &c.options {
                let s = ScrSettings::from_lookup(lookup(&[(c.key, v)]));
                let written = s.to_pairs().into_iter().find(|(k, _)| *k == c.key).unwrap().1;
                assert_eq!(&written, v, "{}={v} survives a round trip", c.key);
            }
        }
        // the current default of each choice is one of its options
        for (k, v) in ScrSettings::default().to_pairs() {
            if let Some(c) = choices().into_iter().find(|c| c.key == k) {
                assert!(c.options.iter().any(|(o, _)| *o == v), "default {k}={v} listed");
            }
        }
    }

    #[test]
    fn fixed_choices_override_the_random_look() {
        let s = ScrSettings {
            layout: Some(Layout::Rings),
            camera: Some(CameraMode::Static),
            color: Some("off".into()),
            wireframe: Some(false),
            extras: Some(Extras::Disk),
            bg_lum: Some(0),
            fps_cap: Some(60),
            budget_pct: 10,
            ..ScrSettings::default()
        };
        for seed in 0..40 {
            let p = plan(&WQHD, seed, &s).unwrap();
            let c = &p.config;
            assert_eq!((c.layout, c.camera, c.color.as_str(), c.wireframe), (Layout::Rings, CameraMode::Static, "off", false));
            assert_eq!(c.show, ["cpu", "ram", "disk", "net", "gpu", "disk-dev"]);
            assert_eq!((c.bg_lum, c.fps_max, p.budget), (0.0, 60.0, 0.1));
            c.resolve_slots().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            if seed > 3 {
                break; // plan() renders; a few seeds are enough
            }
        }
        // colour source fixed while colour is random: never left monochrome
        let s = ScrSettings { color_by: Some(ColorBy::Load), ..ScrSettings::default() };
        for seed in 0..30 {
            let mut c = randomize(&mut fastrand::Rng::with_seed(seed), &WQHD);
            s.apply(&mut c);
            assert!(c.color != "off" && c.color_by == ColorBy::Load);
        }
    }

    #[test]
    fn pixel_shift_moves_one_pixel_within_range_at_random_intervals() {
        let mut p = PixelShift::new(7, 2, 5, 9);
        let mut last = p.offset();
        let mut since = 0;
        let mut gaps = std::collections::HashSet::new();
        for _ in 0..5_000 {
            since += 1;
            if p.tick() {
                let o = p.offset();
                assert_eq!(((o.0 - last.0).abs(), (o.1 - last.1).abs()), (1, 1), "1x1 px step");
                assert!(o.0.abs() <= 2 && o.1.abs() <= 2, "within range: {o:?}");
                assert!((5..=9).contains(&since), "interval {since}");
                gaps.insert(since);
                last = o;
                since = 0;
            } else {
                assert_eq!(p.offset(), last);
            }
        }
        assert!(gaps.len() >= 4, "intervals vary: {gaps:?}");
        let mut one = PixelShift::new(1, 1, 1, 1);
        for _ in 0..50 {
            assert!(one.tick());
            assert!(one.offset().0.abs() <= 1 && one.offset().1.abs() <= 1);
        }
    }
}
