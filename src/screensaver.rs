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
/// Font height range for the budget sizing, pixels (before the density
/// factor).
pub const FONT_MIN: i16 = 8;
pub const FONT_MAX: i16 = 40;
/// Smallest font after the density factor, pixels.
pub const FONT_FLOOR: i16 = 4;
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

/// Divide the budget-sized font height by `density` (glyphs per axis per
/// budget glyph; never below [`FONT_FLOOR`]). Returns (height, est. cols,
/// est. rows).
pub fn apply_density(d: &Display, px: i16, density: u8) -> (i16, u32, u32) {
    let px = ((px as f32 / density.max(1) as f32).round() as i16).max(FONT_FLOOR);
    let (w, h) = (d.width.max(320) as f32, d.height.max(200) as f32);
    (px, (w / (px as f32 * EST_CHAR_ASPECT)).floor() as u32, (h / px as f32).floor() as u32)
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
    match s.levels {
        Levels::L8 => config.ramp = crate::ascii::RAMP_8.to_string(),
        Levels::L16 => config.ramp = crate::ascii::DEFAULT_RAMP.to_string(),
        Levels::L32 => config.ramp = crate::ascii::RAMP_32.to_string(),
    }
    let budget = s.budget_pct as f32 / 100.0;
    let (px, ..) = font_height(d, config.fps_max, spc, budget);
    let (font_px, cols, rows) = apply_density(d, px, s.density);
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

/// How tag text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagText {
    /// Three-row block letters when the text is under [`TAG_BLOCK_PX`] tall.
    Auto,
    Line,
    Blocks,
}

/// Below this text height (pixels) [`TagText::Auto`] uses block letters.
pub const TAG_BLOCK_PX: i16 = 10;

impl TagText {
    pub fn id(self) -> &'static str {
        match self {
            TagText::Auto => "auto",
            TagText::Line => "line",
            TagText::Blocks => "blocks",
        }
    }

    fn parse(v: &str) -> Option<Self> {
        [TagText::Auto, TagText::Line, TagText::Blocks].into_iter().find(|t| t.id() == v)
    }

    /// Block letters for text `font_px` tall?
    pub fn blocks(self, font_px: i16) -> bool {
        match self {
            TagText::Auto => font_px < TAG_BLOCK_PX,
            TagText::Line => false,
            TagText::Blocks => true,
        }
    }
}

/// Number of luminosity levels (glyphs in the ramp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Levels {
    L8,
    L16,
    L32,
}

fn levels_id(l: Levels) -> &'static str {
    match l {
        Levels::L8 => "8",
        Levels::L16 => "16",
        Levels::L32 => "32",
    }
}

fn parse_levels(v: &str) -> Option<Levels> {
    match v {
        // the former 10-level setting maps to the nearest, 8 (`auto`, the
        // former random choice, falls back to the default)
        "10" => Some(Levels::L8),
        _ => [Levels::L8, Levels::L16, Levels::L32].into_iter().find(|l| levels_id(*l) == v),
    }
}

/// The user's choices from the configuration dialog, stored in the
/// screensaver's INI file.
///
/// With `random` on (the default) every run picks its own look and the look
/// fields are ignored; with it off, the look fields are used as they are.
/// The frame-rate cap, CPU budget and burn-in shift always apply.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrSettings {
    /// Master switch: a random look each run.
    pub random: bool,
    // --- the look (used when `random` is off)
    pub layout: Layout,
    pub camera: CameraMode,
    /// `"off"` or a gradient preset id.
    pub color: String,
    pub color_by: ColorBy,
    pub bg_pattern: Pattern,
    pub bg_lum: u8,
    pub wireframe: bool,
    pub stats: bool,
    pub fps_invert: bool,
    pub extras: Extras,
    // --- always applied
    /// Frame-rate cap; `None` = the display's refresh rate.
    pub fps_cap: Option<u32>,
    /// Rendering CPU budget, percent of wall time at full frame rate, for the
    /// font size before the density factor.
    pub budget_pct: u8,
    /// Glyphs per axis relative to the budget-sized font (1 or 2); 2 costs
    /// about 4x the budget.
    pub density: u8,
    pub levels: Levels,
    /// Sound halo: a wireframe 1.2x the size of each solid that grows and
    /// shrinks by ±5% of its radius with the default output device's level.
    pub sound: bool,
    /// Wide halo swing: the halo shrinks and grows an extra ±25% of the
    /// radius (0.9x..1.5x instead of 1.15x..1.25x).
    pub halo_wide: bool,
    /// Frequency gradient: each solid's halo bounces to its own part of the
    /// spectrum, the simplest solid to the bass and the most complex to the
    /// treble; off = every halo follows the overall level.
    pub halo_bands: bool,
    /// Solid tags: each solid's metric floats at a fixed place on screen,
    /// with a line to the solid that stops at its halo or wireframe.
    pub tags: bool,
    /// Tag colour with a colour gradient on (always lifted to stay readable).
    pub tag_color: crate::color::TagColor,
    /// One line, or three-row block letters on small text.
    pub tag_text: TagText,
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
            random: true,
            layout: Layout::Rings,
            camera: CameraMode::Orbit,
            color: "inferno".into(),
            color_by: ColorBy::Lum,
            bg_pattern: Pattern::Checker,
            bg_lum: 2,
            wireframe: false,
            stats: false,
            fps_invert: false,
            extras: Extras::None,
            fps_cap: None,
            budget_pct: (RENDER_BUDGET * 100.0).round() as u8,
            density: 2,
            levels: Levels::L16,
            sound: true,
            halo_wide: false,
            halo_bands: true,
            tags: true,
            tag_color: crate::color::TagColor::Complement,
            tag_text: TagText::Auto,
            shift: true,
            shift_min_frames: 600,
            shift_max_frames: 3600,
            shift_radius: 2,
        }
    }
}

/// INI section name (`szAppName` in the Win32 screensaver sample).
pub const INI_SECTION: &str = "platomon";
pub const SHIFT_FRAMES_MAX: u32 = 1_000_000;
/// Largest burn-in drift from the centred position, pixels per axis.
pub const SHIFT_RADIUS_MAX: u8 = 25;

fn on_off(v: bool) -> String {
    if v { "on" } else { "off" }.to_string()
}

fn parse_on_off(s: &str) -> Option<bool> {
    match s {
        "on" | "1" | "true" | "yes" => Some(true),
        "off" | "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn layout_id(l: Layout) -> &'static str {
    match l {
        Layout::Rings => "rings",
        Layout::Sphere => "sphere",
    }
}

fn camera_id(c: CameraMode) -> &'static str {
    match c {
        CameraMode::Static => "static",
        CameraMode::Orbit => "orbit",
        CameraMode::Focus => "focus",
    }
}

fn color_by_id(c: ColorBy) -> &'static str {
    match c {
        ColorBy::Lum => "lum",
        ColorBy::Load => "load",
    }
}

fn extras_id(e: Extras) -> &'static str {
    match e {
        Extras::None => "none",
        Extras::Nic => "nic",
        Extras::Disk => "disk",
        Extras::Both => "both",
    }
}

fn parse_extras(v: &str) -> Option<Extras> {
    match v {
        "none" => Some(Extras::None),
        "nic" => Some(Extras::Nic),
        "disk" => Some(Extras::Disk),
        "both" => Some(Extras::Both),
        _ => None,
    }
}

/// A dialog drop-down: INI key, label, (INI value, shown text) choices, and
/// whether it is part of the look (greyed out while the look is random).
pub struct Choice {
    pub key: &'static str,
    pub label: &'static str,
    pub options: Vec<(String, String)>,
    pub look: bool,
}

/// Every drop-down in the configuration dialog, in display order.
pub fn choices() -> Vec<Choice> {
    let opt = |v: &str, t: &str| (v.to_string(), t.to_string());
    let on_off = || vec![opt("on", "On"), opt("off", "Off")];
    let mut colors = vec![opt("off", "Off (monochrome)")];
    colors.extend(color::PRESETS.iter().map(|(id, _, _)| opt(id, id)));
    let patterns = Pattern::ALL.iter().map(|p| opt(p.id(), p.id())).collect();
    let lums = (0..=10).map(|l| opt(&l.to_string(), &if l == 0 { "0 (no floor)".into() } else { l.to_string() })).collect();
    let mut caps = vec![opt("display", "Display refresh rate")];
    caps.extend([30, 60, 75, 90, 120, 144, 165, 240].map(|f| opt(&f.to_string(), &format!("{f} fps"))));
    let budgets = [5, 10, 15, 20, 25, 30, 40, 50].map(|b| opt(&b.to_string(), &format!("{b}%"))).to_vec();
    let look = |key, label, options| Choice { key, label, options, look: true };
    let always = |key, label, options| Choice { key, label, options, look: false };
    vec![
        look("layout", "Layout", vec![opt("rings", "Rings"), opt("sphere", "Sphere")]),
        look(
            "camera",
            "Camera",
            vec![opt("static", "Static"), opt("orbit", "Orbit"), opt("focus", "Focus on busiest")],
        ),
        look("color", "Colour", colors),
        look("color_by", "Colour source", vec![opt("lum", "Brightness"), opt("load", "Load per solid")]),
        look("bg_pattern", "Background pattern", patterns),
        look("bg_lum", "Background luminosity", lums),
        look("wireframe", "Wireframe overlay", on_off()),
        look("stats", "Stats bar", on_off()),
        look("fps_invert", "Inverted frame rate", on_off()),
        look(
            "extras",
            "Extra metrics",
            vec![opt("none", "None"), opt("nic", "Network adapter"), opt("disk", "Disk device"), opt("both", "Both")],
        ),
        always("fps_cap", "Frame-rate cap", caps),
        always("budget", "CPU budget", budgets),
        always("density", "Text density", vec![opt("1", "Standard"), opt("2", "Double (about 4x CPU)")]),
        always(
            "levels",
            "Luminosity levels",
            vec![opt("8", "8"), opt("16", "16"), opt("32", "32")],
        ),
        always("sound", "Sound halo (output device)", on_off()),
        always(
            "halo_wide",
            "Halo size swing",
            vec![opt("off", "\u{b1}5% of radius"), opt("on", "\u{b1}30% of radius (extra \u{b1}25%)")],
        ),
        always(
            "halo_bands",
            "Halo bounce source",
            vec![
                opt("on", "Frequency by solid (simple = bass, complex = treble)"),
                opt("off", "Overall level (all solids together)"),
            ],
        ),
        always("tags", "Solid tags (metric + leader line)", on_off()),
        always(
            "tag_color",
            "Tag colour",
            crate::color::TagColor::CHOICES.iter().map(|c| opt(&c.id(), c.describe())).collect(),
        ),
        always(
            "tag_text",
            "Tag text",
            vec![
                opt("auto", "Block letters when text is under 10 px"),
                opt("line", "Always one line"),
                opt("blocks", "Always block letters (3 lines)"),
            ],
        ),
        always(
            "shift_radius",
            "Burn-in shift range",
            (1..=SHIFT_RADIUS_MAX).map(|r| opt(&r.to_string(), &format!("\u{b1}{r} px"))).collect(),
        ),
    ]
}

impl ScrSettings {
    /// `(key, value)` pairs as written to the INI file.
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("random", on_off(self.random)),
            ("layout", layout_id(self.layout).into()),
            ("camera", camera_id(self.camera).into()),
            ("color", self.color.clone()),
            ("color_by", color_by_id(self.color_by).into()),
            ("bg_pattern", self.bg_pattern.id().into()),
            ("bg_lum", self.bg_lum.to_string()),
            ("wireframe", on_off(self.wireframe)),
            ("stats", on_off(self.stats)),
            ("fps_invert", on_off(self.fps_invert)),
            ("extras", extras_id(self.extras).into()),
            ("fps_cap", self.fps_cap.map_or_else(|| "display".to_string(), |f| f.to_string())),
            ("budget", self.budget_pct.to_string()),
            ("density", self.density.to_string()),
            ("levels", levels_id(self.levels).into()),
            ("sound", on_off(self.sound)),
            ("halo_wide", on_off(self.halo_wide)),
            ("halo_bands", on_off(self.halo_bands)),
            ("tags", on_off(self.tags)),
            ("tag_color", self.tag_color.id()),
            ("tag_text", self.tag_text.id().into()),
            ("shift", on_off(self.shift)),
            ("shift_min_frames", self.shift_min_frames.to_string()),
            ("shift_max_frames", self.shift_max_frames.to_string()),
            ("shift_radius", self.shift_radius.to_string()),
        ]
    }

    /// Read settings through `get(key)`; missing or invalid values fall back
    /// to the defaults, one key at a time.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        let val = |k: &str| get(k).map(|v| v.trim().to_ascii_lowercase()).filter(|v| !v.is_empty());
        let flag = |k: &str, def: bool| val(k).and_then(|v| parse_on_off(&v)).unwrap_or(def);
        let num = |k: &str| val(k).and_then(|v| v.parse::<u32>().ok());
        let shift_min = num("shift_min_frames").filter(|v| (1..=SHIFT_FRAMES_MAX).contains(v)).unwrap_or(d.shift_min_frames);
        let shift_max = num("shift_max_frames")
            .filter(|v| (1..=SHIFT_FRAMES_MAX).contains(v))
            .unwrap_or(d.shift_max_frames)
            .max(shift_min);
        Self {
            random: flag("random", d.random),
            layout: val("layout").and_then(|v| v.parse().ok()).unwrap_or(d.layout),
            camera: val("camera").and_then(|v| v.parse().ok()).unwrap_or(d.camera),
            color: val("color")
                .filter(|v| v == "off" || color::PRESETS.iter().any(|(id, _, _)| id == v))
                .unwrap_or(d.color.clone()),
            color_by: val("color_by").and_then(|v| v.parse().ok()).unwrap_or(d.color_by),
            bg_pattern: val("bg_pattern")
                .and_then(|v| Pattern::ALL.into_iter().find(|p| p.id() == v))
                .unwrap_or(d.bg_pattern),
            bg_lum: num("bg_lum").filter(|l| *l <= 10).map_or(d.bg_lum, |l| l as u8),
            wireframe: flag("wireframe", d.wireframe),
            stats: flag("stats", d.stats),
            fps_invert: flag("fps_invert", d.fps_invert),
            extras: val("extras").and_then(|v| parse_extras(&v)).unwrap_or(d.extras),
            fps_cap: match val("fps_cap").as_deref() {
                None | Some("display") => None,
                Some(v) => v.parse::<u32>().ok().filter(|f| (30..=240).contains(f)).or(d.fps_cap),
            },
            budget_pct: num("budget").filter(|b| (1..=100).contains(b)).map_or(d.budget_pct, |b| b as u8),
            density: num("density").filter(|v| (1..=2).contains(v)).map_or(d.density, |v| v as u8),
            levels: val("levels").and_then(|v| parse_levels(&v)).unwrap_or(d.levels),
            sound: flag("sound", d.sound),
            halo_wide: flag("halo_wide", d.halo_wide),
            halo_bands: flag("halo_bands", d.halo_bands),
            tags: flag("tags", d.tags),
            tag_color: val("tag_color").and_then(|v| v.parse().ok()).unwrap_or(d.tag_color),
            tag_text: val("tag_text").and_then(|v| TagText::parse(&v)).unwrap_or(d.tag_text),
            shift: flag("shift", d.shift),
            shift_min_frames: shift_min,
            shift_max_frames: shift_max,
            shift_radius: num("shift_radius")
                .filter(|r| (1..=SHIFT_RADIUS_MAX as u32).contains(r))
                .map_or(d.shift_radius, |r| r as u8),
        }
    }

    /// Unless the look is random, replace the random look with the chosen one.
    pub fn apply(&self, c: &mut Config) {
        if self.random {
            return;
        }
        c.layout = self.layout;
        c.camera = self.camera;
        c.color = self.color.clone();
        c.color_by = self.color_by;
        c.bg_pattern = self.bg_pattern;
        c.bg_lum = self.bg_lum as f32;
        c.wireframe = self.wireframe;
        c.show_stats = self.stats;
        c.fps_invert = self.fps_invert;
        let mut show: Vec<String> = ["cpu", "ram", "disk", "net", "gpu"].map(String::from).to_vec();
        if matches!(self.extras, Extras::Nic | Extras::Both) {
            show.push("nic".into());
        }
        if matches!(self.extras, Extras::Disk | Extras::Both) {
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
    fn double_density_halves_the_font() {
        let (px, cols, rows) = font_height(&WQHD, 144.0, 2e-7, RENDER_BUDGET);
        let (px1, cols1, rows1) = apply_density(&WQHD, px, 1);
        assert_eq!((px1, cols1, rows1), (px, cols, rows), "density 1 is unchanged");
        let (px2, cols2, rows2) = apply_density(&WQHD, 20, 2);
        assert_eq!(px2, 10);
        assert_eq!((cols2, rows2), (465, 144));
        assert_eq!(apply_density(&WQHD, FONT_MIN, 2).0, FONT_FLOOR);
        assert_eq!(apply_density(&WQHD, 5, 2).0, FONT_FLOOR, "never below the floor");
        // the plan uses it: density 2 ⇒ half the font (or the floor) of the
        // budget size from the plan's own speed test (two separate speed
        // tests can differ, so compare against the same measurement)
        let two = plan(&WQHD, 5, &ScrSettings::default()).unwrap();
        let (budget_px, ..) = font_height(&WQHD, two.config.fps_max, two.sec_per_cell, two.budget);
        let one = apply_density(&WQHD, budget_px, 1);
        assert_eq!(two.font_px, ((budget_px as f32 / 2.0).round() as i16).max(FONT_FLOOR));
        assert!(two.font_px == FONT_FLOOR || (two.est_grid.0 > one.1 && two.est_grid.1 > one.2));
    }

    #[test]
    fn luminosity_levels_pick_the_ramp() {
        let ramp32 = crate::ascii::Ramp::new(crate::ascii::RAMP_32);
        assert_eq!(ramp32.len(), 32);
        let distinct: std::collections::HashSet<char> = crate::ascii::RAMP_32.chars().collect();
        assert_eq!(distinct.len(), 32, "32 different glyphs");
        assert!(crate::ascii::RAMP_32.starts_with(' '), "darkest is blank");
        for random in [true, false] {
            for (l, n) in [(Levels::L8, 8), (Levels::L16, 16), (Levels::L32, 32)] {
                let p = plan(&WQHD, 2, &ScrSettings { levels: l, random, ..ScrSettings::default() }).unwrap();
                assert_eq!(crate::ascii::Ramp::new(&p.config.ramp).len(), n, "{l:?}");
            }
        }
        // the 8-level ramp: distinct glyphs, the classic ramp's order, blank to @
        let r8: Vec<char> = crate::ascii::RAMP_8.chars().collect();
        assert_eq!(r8.iter().collect::<std::collections::HashSet<_>>().len(), 8);
        let pos: Vec<usize> = r8.iter().map(|c| crate::ascii::CLASSIC_RAMP.chars().position(|d| d == *c).unwrap()).collect();
        assert!(pos.windows(2).all(|p| p[0] < p[1]) && pos[0] == 0 && pos[7] == 9, "{pos:?}");
        // no random choice any more: the default is 16 levels, every run
        for seed in 0..20 {
            let p = plan(&WQHD, seed, &ScrSettings::default()).unwrap();
            assert_eq!(p.config.ramp, crate::ascii::DEFAULT_RAMP, "seed {seed}");
        }
        // older INI values: `auto` falls back to the default, `10` becomes 8
        let s = ScrSettings::from_lookup(|k| (k == "levels").then(|| "auto".to_string()));
        assert_eq!(s.levels, Levels::L16);
        let s = ScrSettings::from_lookup(|k| (k == "levels").then(|| "10".to_string()));
        assert_eq!(s.levels, Levels::L8);
        // tag text: block letters below 10 px in auto, and as chosen otherwise
        assert!(TagText::Auto.blocks(9) && !TagText::Auto.blocks(10));
        assert!(TagText::Blocks.blocks(20) && !TagText::Line.blocks(4));
        let s = ScrSettings::from_lookup(|k| (k == "tag_text").then(|| "blocks".to_string()));
        assert_eq!(s.tag_text, TagText::Blocks);
        assert_eq!(ScrSettings::default().tag_text, TagText::Auto);
        // the dialog offers 8, 16 and 32 only
        let lv = choices().into_iter().find(|c| c.key == "levels").unwrap();
        let ids: Vec<&str> = lv.options.iter().map(|o| o.0.as_str()).collect();
        assert_eq!(ids, ["8", "16", "32"]);
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
        assert_eq!((d.random, d.shift, d.budget_pct, d.fps_cap), (true, true, 15, None));
        let s = ScrSettings {
            random: false,
            layout: Layout::Sphere,
            camera: CameraMode::Focus,
            color: "viridis".into(),
            color_by: ColorBy::Load,
            bg_pattern: Pattern::Hex,
            bg_lum: 0,
            wireframe: true,
            stats: false,
            fps_invert: true,
            extras: Extras::Both,
            fps_cap: Some(60),
            budget_pct: 25,
            density: 1,
            levels: Levels::L32,
            sound: false,
            halo_wide: true,
            halo_bands: false,
            tags: false,
            tag_color: crate::color::TagColor::Theme,
            tag_text: TagText::Blocks,
            shift: false,
            shift_min_frames: 10,
            shift_max_frames: 20,
            shift_radius: 3,
        };
        let pairs = s.to_pairs();
        let back = ScrSettings::from_lookup(|k| pairs.iter().find(|(pk, _)| *pk == k).map(|(_, v)| v.clone()));
        assert_eq!(back, s);
        // junk falls back per key (old per-setting "random" values included);
        // max < min is raised to min
        let j = ScrSettings::from_lookup(lookup(&[
            ("layout", "random"),
            ("color", "plaid"),
            ("bg_lum", "11"),
            ("fps_cap", "5"),
            ("budget", "0"),
            ("shift_min_frames", "500"),
            ("shift_max_frames", "100"),
            ("wireframe", "ON"),
            ("random", "off"),
        ]));
        assert_eq!((j.layout, j.color.as_str(), j.bg_lum, j.fps_cap, j.budget_pct), (d.layout, "inferno", 2, None, 15));
        assert_eq!((j.shift_min_frames, j.shift_max_frames, j.wireframe, j.random), (500, 500, true, false));
    }

    #[test]
    fn every_dialog_choice_parses() {
        let keys: Vec<&str> = ScrSettings::default().to_pairs().iter().map(|(k, _)| *k).collect();
        let looks: Vec<&str> = choices().iter().filter(|c| c.look).map(|c| c.key).collect();
        assert_eq!(
            looks,
            ["layout", "camera", "color", "color_by", "bg_pattern", "bg_lum", "wireframe", "stats", "fps_invert", "extras"]
        );
        for c in choices() {
            assert!(keys.contains(&c.key), "{} is a settings key", c.key);
            assert!(c.options.iter().all(|(v, _)| v != "random"), "{}: no per-setting random", c.key);
            for (v, _) in &c.options {
                let s = ScrSettings::from_lookup(lookup(&[(c.key, v)]));
                let written = s.to_pairs().into_iter().find(|(k, _)| *k == c.key).unwrap().1;
                assert_eq!(&written, v, "{}={v} survives a round trip", c.key);
            }
        }
        // the default of each choice is one of its options
        for (k, v) in ScrSettings::default().to_pairs() {
            if let Some(c) = choices().into_iter().find(|c| c.key == k) {
                assert!(c.options.iter().any(|(o, _)| *o == v), "default {k}={v} listed");
            }
        }
    }

    #[test]
    fn master_toggle_decides_between_random_and_chosen_look() {
        let chosen = ScrSettings {
            random: false,
            layout: Layout::Rings,
            camera: CameraMode::Static,
            color: "off".into(),
            wireframe: false,
            extras: Extras::Disk,
            bg_lum: 0,
            fps_cap: Some(60),
            budget_pct: 10,
            ..ScrSettings::default()
        };
        for seed in 0..3 {
            let p = plan(&WQHD, seed, &chosen).unwrap();
            let c = &p.config;
            assert_eq!((c.layout, c.camera, c.color.as_str(), c.wireframe), (Layout::Rings, CameraMode::Static, "off", false));
            assert_eq!((c.bg_pattern, c.show_stats, c.fps_invert), (Pattern::Checker, false, false));
            assert_eq!(c.show, ["cpu", "ram", "disk", "net", "gpu", "disk-dev"]);
            assert_eq!((c.bg_lum, c.fps_max, p.budget), (0.0, 60.0, 0.1));
            c.resolve_slots().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        }
        // master toggle on: the look fields are ignored, cap and budget still apply
        let random = ScrSettings { random: true, ..chosen.clone() };
        let mut looks = std::collections::HashSet::new();
        for seed in 0..40 {
            let mut c = randomize(&mut fastrand::Rng::with_seed(seed), &WQHD);
            let before = c.clone();
            random.apply(&mut c);
            assert_eq!(c, before, "random look untouched");
            looks.insert(format!("{:?}{:?}{}", c.layout, c.camera, c.color));
        }
        assert!(looks.len() > 10, "varied looks: {}", looks.len());
        let p = plan(&WQHD, 1, &random).unwrap();
        assert_eq!((p.config.fps_max, p.budget), (60.0, 0.1));
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

    #[test]
    fn shift_range_goes_up_to_25_px() {
        // every range 1..=25 is offered and survives the INI round trip
        let c = choices().into_iter().find(|c| c.key == "shift_radius").unwrap();
        assert_eq!(c.options.len(), 25);
        assert_eq!(c.options.last().unwrap().1, "\u{b1}25 px");
        assert_eq!(ScrSettings::from_lookup(lookup(&[("shift_radius", "25")])).shift_radius, 25);
        assert_eq!(ScrSettings::from_lookup(lookup(&[("shift_radius", "26")])).shift_radius, 2, "beyond the max: default");
        // a 25 px range drifts that far and no further, 1 px per axis per step
        let mut p = PixelShift::new(3, 25, 1, 1);
        let (mut max_x, mut max_y) = (0, 0);
        let mut last = p.offset();
        for _ in 0..200_000 {
            assert!(p.tick());
            let o = p.offset();
            assert_eq!(((o.0 - last.0).abs(), (o.1 - last.1).abs()), (1, 1));
            assert!(o.0.abs() <= 25 && o.1.abs() <= 25, "{o:?}");
            (max_x, max_y) = (max_x.max(o.0.abs()), max_y.max(o.1.abs()));
            last = o;
        }
        assert_eq!((max_x, max_y), (25, 25), "reaches the edge of the range");
    }
}
