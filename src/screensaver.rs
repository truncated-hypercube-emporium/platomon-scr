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
use crate::scene::{AnimParams, Layout, OrbitCenter, Ordering, Scene, SlotLayout};
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
/// Smallest font after the density factor, pixels (low enough that every
/// rendering resolution up to Industrial Boiler, 6x, still differs).
pub const FONT_FLOOR: i16 = 2;
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
        spp: SCR_SPP,
        bounces: SCR_BOUNCES,
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
    c.layout = match rng.u8(0..20) {
        0..=5 => Layout::Rings,
        6..=11 => Layout::Sphere,
        12..=14 => Layout::Line,
        15..=16 => Layout::Stack,
        _ => Layout::Orbit,
    };
    // orbits of their own each run (the Layout dialog's sliders override)
    c.orbit_velocity = round2(range(rng, 0.2, 0.6));
    c.orbit_inclination = range(rng, 0.0, 45.0).round();
    c.orbit_eccentricity = round2(range(rng, 0.0, 0.4));
    c.orbit_variation = round2(range(rng, 0.1, 0.7));

    // camera
    c.camera = match rng.u8(0..20) {
        0..=2 => CameraMode::Static,
        3..=8 => CameraMode::Orbit,
        9..=13 => CameraMode::Focus,
        14..=16 => CameraMode::Tilt,
        _ => CameraMode::Resonance,
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
    c.bg_waves = rng.u32(5..=12);
    c.bg_wave_speed = (range(rng, 0.2, 1.2) * 10.0).round() / 10.0;

    // colour
    if rng.u8(0..10) >= 3 {
        let presets: Vec<&str> = color::PRESETS.iter().map(|p| p.0).filter(|id| *id != "grey").collect();
        c.color = presets[rng.usize(0..presets.len())].to_string();
        c.color_by = match rng.u8(0..20) {
            0..=7 => ColorBy::Lum,
            8..=12 => ColorBy::Load,
            13..=15 => ColorBy::Spin,
            _ => ColorBy::Sound,
        };
        c.bg_color = match rng.u8(0..20) {
            0..=11 => color::BG_COLOR_DEFAULT.to_string(),
            12..=14 => "match".to_string(),
            15..=16 => "off".to_string(),
            _ => presets[rng.usize(0..presets.len())].to_string(),
        };
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
        auto_distance: cfg.auto_distance,
    }
}

/// The scene for `cfg`'s resolved slots, with its layout and background.
pub fn build_scene(cfg: &Config, slots: &[SlotSpec]) -> Scene {
    let layouts: Vec<SlotLayout> = slots.iter().map(|s| SlotLayout { solid: s.solid, ring: s.ring }).collect();
    Scene::arranged(&layouts, &anim_params(cfg), &camera_params(cfg), &cfg.arrangement(slots)).with_ground(&cfg.ground_params())
}

/// Render settings for `cfg` (no picture-ratio framing: the screen is filled).
pub fn render_settings(cfg: &Config, char_aspect: f32) -> RenderSettings {
    RenderSettings {
        spp: cfg.spp,
        trace: TraceSettings { bounces: cfg.bounces, max_steps: cfg.max_steps, max_dist: cfg.max_dist, ..TraceSettings::default() }
            .with_far_ground(cfg.far_ground),
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
    let config = plan_config(d, seed, s);
    let spc = seconds_per_cell(&config)?;
    let budget = s.budget();
    let (px, ..) = font_height(d, config.fps_max, spc, budget);
    let (font_px, cols, rows) = apply_density(d, px, s.density);
    Ok(Plan { config, font_px, est_grid: (cols, rows), sec_per_cell: spc, budget, settings: s.clone() })
}

/// The settings a running preview cannot pick up: they set the text size
/// (CPU budget, density, and the frame-rate cap the budget is spread
/// over), which is fixed once its window is made.
pub const PREVIEW_RESTART_KEYS: [&str; 4] = ["budget", "overdrive", "density", "fps_cap"];

/// Whether `edited` changes a setting the preview started with `running`
/// cannot apply live (see [`PREVIEW_RESTART_KEYS`]).
pub fn preview_needs_restart(running: &ScrSettings, edited: &ScrSettings) -> bool {
    (running.budget_pct, running.overdrive, running.density, running.fps_cap)
        != (edited.budget_pct, edited.overdrive, edited.density, edited.fps_cap)
}

/// `edited` as a running preview applies it: everything but the
/// [`PREVIEW_RESTART_KEYS`], which stay as `running` had them.
pub fn preview_live(running: &ScrSettings, edited: &ScrSettings) -> ScrSettings {
    ScrSettings {
        budget_pct: running.budget_pct,
        overdrive: running.overdrive,
        density: running.density,
        fps_cap: running.fps_cap,
        ..edited.clone()
    }
}

/// Settings as the preview file holds them: one `key=value` per line.
pub fn preview_text(s: &ScrSettings) -> String {
    s.to_pairs().into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k}={v}\n")).collect()
}

/// Settings from [`preview_text`] (defaults for anything missing).
pub fn preview_settings(text: &str) -> ScrSettings {
    let pairs: Vec<(&str, &str)> = text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim(), v.trim())).collect();
    ScrSettings::from_lookup(|k| pairs.iter().find(|(pk, _)| *pk == k).map(|(_, v)| v.to_string()))
}

/// The run's config for `s`: [`plan`] without the speed test and text size
/// (a running preview re-plans with this when the settings change; the
/// same seed keeps the same random look).
pub fn plan_config(d: &Display, seed: u64, s: &ScrSettings) -> Config {
    let mut rng = fastrand::Rng::with_seed(seed);
    let mut config = randomize(&mut rng, d);
    s.apply(&mut config);
    // A random look colours by sound only when the sound halo (and so the
    // output-device meter) is on.
    if s.random && !s.sound && config.color_by == ColorBy::Sound {
        config.color_by = ColorBy::Load;
    }
    config.colors = s.colors;
    config.auto_distance = s.auto_distance;
    config.far_ground = s.far_ground;
    config.tag_fixed_len = s.tag_fixed;
    config.halo_color = s.halo_color;
    config.ordering = s.ordering;
    config.orbit_center = s.orbit_center;
    config.adaptive_quality = s.adaptive;
    for &(k, v) in &s.tuning {
        set_tunable(&mut config, k, v);
    }
    config.fps_max = match s.fps_cap {
        Some(f) => (f as f32).clamp(FPS_CAP.0, FPS_CAP.1),
        None => fps_for(d),
    };
    config.sanitize();
    match s.levels {
        Levels::L8 => config.ramp = crate::ascii::RAMP_8.to_string(),
        Levels::L16 => config.ramp = crate::ascii::DEFAULT_RAMP.to_string(),
        Levels::L32 => config.ramp = crate::ascii::RAMP_32.to_string(),
    }
    config
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
    /// Background colouring, independent of the solids: `auto`, `off`,
    /// `match` or a gradient preset id.
    pub bg_color: String,
    pub bg_pattern: Pattern,
    pub bg_lum: u8,
    /// Quasicrystal background: plane waves (its rotational symmetry).
    pub bg_waves: u32,
    /// Quasicrystal background: wave phase speed, radians/s (0 = still).
    pub bg_wave_speed: f32,
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
    /// Overdrive: ignore `budget_pct` and let rendering take all the CPU
    /// time it wants (text sized for the whole CPU, frames back to back).
    pub overdrive: bool,
    /// Glyphs per axis relative to the budget-sized font (1 or 2); 2 costs
    /// about 4x the budget.
    pub density: u8,
    pub levels: Levels,
    /// Palette size: 16, 32, 64 or 128 colours per gradient.
    pub colors: usize,
    /// Sound halo: a wireframe 1.2x the size of each solid that grows and
    /// shrinks by ±5% of its radius with the default output device's level.
    pub sound: bool,
    /// Halo size swing, percent of the radius either way: 10 (1.1x..1.3x),
    /// 30 (0.9x..1.5x) or 60 (0.6x..1.8x).
    pub halo_swing: u8,
    /// Frequency gradient: each solid's halo bounces to its own part of the
    /// spectrum, the simplest solid to the bass and the most complex to the
    /// treble; off = every halo follows the overall level.
    pub halo_bands: bool,
    /// What the halo's colour comes from (with a colour gradient on).
    pub halo_color: color::HaloColor,
    /// Which solid takes which place in the layout.
    pub ordering: Ordering,
    /// Orbit layout: the solid the others orbit.
    pub orbit_center: OrbitCenter,
    /// Adaptive quality: drop samples, then bounces, when frames run late.
    pub adaptive: bool,
    /// Dynamic camera distance: move in and out to keep every solid in frame.
    pub auto_distance: bool,
    /// Extended floor: the ground plane drawn 8x further out.
    pub far_ground: bool,
    /// Fixed tag length: leader lines keep one length.
    pub tag_fixed: bool,
    /// Solid tags: each solid's metric floats at a fixed place on screen,
    /// with a line to the solid that stops at its halo or wireframe.
    pub tags: bool,
    /// Tag colour with a colour gradient on (always lifted to stay readable).
    pub tag_color: crate::color::TagColor,
    /// One line, or three-row block letters on small text.
    pub tag_text: TagText,
    /// Tuning overrides from the Tuning dialog: (key, value) for each
    /// [`TUNABLES`] entry the user has set, applied every run.
    pub tuning: Vec<(&'static str, f32)>,
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
            bg_color: color::BG_COLOR_DEFAULT.into(),
            bg_pattern: Pattern::Checker,
            bg_lum: 2,
            bg_waves: crate::scene::ground::WAVES_DEFAULT,
            bg_wave_speed: crate::scene::ground::WAVE_SPEED_DEFAULT,
            wireframe: false,
            stats: false,
            fps_invert: false,
            extras: Extras::None,
            fps_cap: None,
            budget_pct: (RENDER_BUDGET * 100.0).round() as u8,
            overdrive: false,
            density: 2,
            levels: Levels::L16,
            colors: color::DEFAULT_COLORS,
            sound: true,
            halo_swing: 10,
            halo_bands: true,
            halo_color: color::HaloColor::Under,
            ordering: Ordering::Listed,
            orbit_center: OrbitCenter::Cpu,
            adaptive: true,
            auto_distance: false,
            far_ground: false,
            tag_fixed: false,
            tags: true,
            tag_color: crate::color::TagColor::Complement,
            tag_text: TagText::Auto,
            tuning: Vec::new(),
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
/// Halo size swings offered, percent of the radius either way.
pub const HALO_SWING_PCT: [u8; 3] = [10, 30, 60];

/// The secondary dialogs of the settings dialog: which one shows a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Tuning,
    Layout,
    Rendering,
}

/// A numeric setting a secondary dialog edits with a slider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tunable {
    /// The dialog it is in.
    pub panel: Panel,
    /// Config field.
    pub key: &'static str,
    /// INI key (`tune_<field>`).
    pub ini: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    /// Tooltip text.
    pub help: &'static str,
}

impl Tunable {
    /// Number of steps from `min` to `max` (slider positions 0..=steps).
    pub fn steps(&self) -> u32 {
        ((self.max - self.min) / self.step).round() as u32
    }

    pub fn value_at(&self, pos: u32) -> f32 {
        let v = self.min + self.step * pos.min(self.steps()) as f32;
        // tidy float noise (0.30000001 → 0.3)
        (v * 1000.0).round() / 1000.0
    }

    pub fn pos_of(&self, v: f32) -> u32 {
        (((v - self.min) / self.step).round().max(0.0) as u32).min(self.steps())
    }

    /// `v` clamped to the range and rounded to a step.
    pub fn snap(&self, v: f32) -> f32 {
        self.value_at(self.pos_of(v))
    }

    /// `v` as shown beside the slider.
    pub fn show(&self, v: f32) -> String {
        if self.step >= 1.0 {
            format!("{v:.0}")
        } else if self.step >= 0.1 {
            format!("{v:.1}")
        } else {
            format!("{v:.2}")
        }
    }
}

macro_rules! tunable {
    ($panel:ident, $key:literal, $label:literal, $min:expr, $max:expr, $step:expr, $help:literal) => {
        Tunable {
            panel: Panel::$panel,
            key: $key,
            ini: concat!("tune_", $key),
            label: $label,
            min: $min,
            max: $max,
            step: $step,
            help: $help,
        }
    };
}

/// Every numeric setting set with a slider, in the Tuning, Layout and
/// Rendering dialogs ([`Tunable::panel`]). A set value applies every run
/// (random look or not); unset ones keep the random or default value.
pub const TUNABLES: [Tunable; 23] = [
    tunable!(Tuning, "orbit_period", "Orbit period (s)", 15.0, 180.0, 5.0, "Seconds per camera orbit (orbit, tilt and resonance cameras). Tilt also takes one period per rise or fall."),
    tunable!(Tuning, "orbit_bob", "Orbit bob", 0.0, 1.0, 0.05, "How far the orbiting camera rises and falls twice per turn, in world units."),
    tunable!(Tuning, "focus_zoom", "Focus zoom", 0.0, 1.0, 0.05, "Focus camera: 0 only turns toward the busiest solid, 1 comes in to a close-up."),
    tunable!(Tuning, "focus_tau", "Focus glide (s)", 0.25, 5.0, 0.25, "Focus camera: how long its glides take (smoothing time constant)."),
    tunable!(Tuning, "bg_contrast", "Floor contrast", 0.0, 1.0, 0.05, "How far the floor pattern's tones spread around its brightness."),
    tunable!(Tuning, "bg_scale", "Floor tile size", 0.3, 4.0, 0.1, "Size of the floor pattern's tiles (the quasicrystal's wavelength), in world units."),
    tunable!(Tuning, "bg_sway", "Floor sway", 0.0, 2.0, 0.05, "How far the floor pattern sways in a slow circle; 0 holds it still."),
    tunable!(Tuning, "bg_sway_period", "Sway period (s)", 3.0, 30.0, 1.0, "Seconds per floor sway cycle."),
    tunable!(Tuning, "bg_rotate_period", "Floor turn period (s)", 10.0, 180.0, 5.0, "Seconds per turn when the floor pattern rotates."),
    tunable!(Tuning, "min_scale", "Smallest solid size", 0.2, 1.0, 0.05, "Size of a solid whose metric is idle, as a share of full size."),
    tunable!(Tuning, "max_scale", "Largest solid size", 0.5, 1.5, 0.05, "Size of a solid whose metric is at full load."),
    tunable!(Tuning, "idle_spin", "Idle spin (rad/s)", 0.0, 1.0, 0.05, "How fast a solid spins when its metric is idle."),
    tunable!(Tuning, "max_spin", "Fastest spin (rad/s)", 0.5, 8.0, 0.25, "How fast a solid spins at full load."),
    tunable!(Tuning, "exposure", "Exposure", 0.3, 3.0, 0.1, "Overall picture brightness before it is turned into glyphs."),
    tunable!(Layout, "spacing", "Spacing", 0.8, 2.5, 0.05, "Distance between neighbouring solids (and between rings, shells or orbits), as a multiple of the standard spacing."),
    tunable!(Layout, "orbit_velocity", "Orbit velocity", 0.05, 2.0, 0.05, "Orbit layout: how fast the innermost orbit goes round, in radians per second (0.35 is about 18 s per turn). Outer orbits take longer."),
    tunable!(Layout, "orbit_inclination", "Orbital inclination", 0.0, 90.0, 5.0, "Orbit layout: how far the orbits tilt from the floor, in degrees (0 = flat, 90 = upright)."),
    tunable!(Layout, "orbit_eccentricity", "Orbital eccentricity", 0.0, 0.8, 0.05, "Orbit layout: 0 = circular orbits, higher = longer ellipses, the solids speeding up as they swing close to the centre."),
    tunable!(Layout, "orbit_variation", "Orbital variation", 0.0, 1.0, 0.05, "Orbit layout: 0 = every orbit alike, higher = each orbit gets its own spacing, speed, tilt, shape and direction."),
    tunable!(Rendering, "spp", "Samples per cell", 1.0, 4.0, 1.0, "Light paths traced per character cell: more is smoother and less noisy, and costs that many times the work."),
    tunable!(Rendering, "bounces", "Light bounces", 0.0, 4.0, 1.0, "Indirect bounces after the first hit: 0 gives hard direct light only, more fills shadows with reflected light at more cost."),
    tunable!(Rendering, "max_steps", "March steps", 16.0, 256.0, 8.0, "Most sphere-tracing steps per ray: fewer is faster but can miss thin edges and grazing surfaces."),
    tunable!(Rendering, "max_dist", "Render distance", 20.0, 200.0, 10.0, "How far rays travel before they count as sky, in world units; the extended floor multiplies it."),
];

/// Set config field `key` (a [`TUNABLES`] key) to `v`.
pub fn set_tunable(c: &mut Config, key: &str, v: f32) {
    match key {
        "orbit_period" => c.orbit_period = v,
        "orbit_bob" => c.orbit_bob = v,
        "focus_zoom" => c.focus_zoom = v,
        "focus_tau" => c.focus_tau = v,
        "bg_contrast" => c.bg_contrast = v,
        "bg_scale" => c.bg_scale = v,
        "bg_sway" => c.bg_sway = v,
        "bg_sway_period" => c.bg_sway_period = v,
        "bg_rotate_period" => c.bg_rotate_period = v,
        "min_scale" => c.min_scale = v,
        "max_scale" => c.max_scale = v,
        "idle_spin" => c.idle_spin = v,
        "max_spin" => c.max_spin = v,
        "exposure" => c.exposure = v,
        "spacing" => c.spacing = v,
        "orbit_velocity" => c.orbit_velocity = v,
        "orbit_inclination" => c.orbit_inclination = v,
        "orbit_eccentricity" => c.orbit_eccentricity = v,
        "orbit_variation" => c.orbit_variation = v,
        "spp" => c.spp = v.round().max(1.0) as u32,
        "bounces" => c.bounces = v.round().max(0.0) as u32,
        "max_steps" => c.max_steps = v.round().max(1.0) as u32,
        "max_dist" => c.max_dist = v,
        _ => {}
    }
}

/// The screensaver's own cheap rendering (every run, unless the Rendering
/// dialog sets otherwise): samples per cell and bounces.
pub const SCR_SPP: u32 = 1;
pub const SCR_BOUNCES: u32 = 1;

/// Tunable `key`'s value while it is on auto: the default, with the
/// screensaver's own rendering settings.
pub fn tunable_auto(key: &str) -> f32 {
    get_tunable(&Config { spp: SCR_SPP, bounces: SCR_BOUNCES, ..Config::default() }, key)
}

/// Config field `key`'s value (a [`TUNABLES`] key).
pub fn get_tunable(c: &Config, key: &str) -> f32 {
    match key {
        "orbit_period" => c.orbit_period,
        "orbit_bob" => c.orbit_bob,
        "focus_zoom" => c.focus_zoom,
        "focus_tau" => c.focus_tau,
        "bg_contrast" => c.bg_contrast,
        "bg_scale" => c.bg_scale,
        "bg_sway" => c.bg_sway,
        "bg_sway_period" => c.bg_sway_period,
        "bg_rotate_period" => c.bg_rotate_period,
        "min_scale" => c.min_scale,
        "max_scale" => c.max_scale,
        "idle_spin" => c.idle_spin,
        "max_spin" => c.max_spin,
        "exposure" => c.exposure,
        "spacing" => c.spacing,
        "orbit_velocity" => c.orbit_velocity,
        "orbit_inclination" => c.orbit_inclination,
        "orbit_eccentricity" => c.orbit_eccentricity,
        "orbit_variation" => c.orbit_variation,
        "spp" => c.spp as f32,
        "bounces" => c.bounces as f32,
        "max_steps" => c.max_steps as f32,
        "max_dist" => c.max_dist,
        _ => f32::NAN,
    }
}

/// Tooltip text for a dialog setting (`key` as in [`choices`], plus the
/// dialog's own controls); empty when there is none.
pub fn help(key: &str) -> &'static str {
    match key {
        "random" => "Random start. On: every run starts with a new random look (layout, camera, colours, background...). Off: the look chosen in this group is used every time.",
        "reroll" => "Roll random values into every setting of this group and into the Tuning dialog's sliders now (and turn Random start off so they are kept). Nothing is saved until OK or Apply.",
        "layout" => "Rings: the solids stand in rings on the floor. Sphere: they float on concentric spherical shells. Line: one evenly spaced row. Stack: one above another in the centre. Orbit: one solid in the centre and the others orbiting it. Ordering, spacing and the orbits are in the Layout dialog.",
        "orbit_center" => "Orbit layout: the solid the others orbit. CPU, RAM, Network or Disk: that metric's solid. Size: whichever solid is largest at the moment. Spin: whichever spins fastest. A new centre glides in smoothly.",
        "ordering" => "Which solid takes which place: in metric order, from the simplest solid (fewest edges) to the most complex, the reverse, or shuffled. The first place is the inner ring's front, the inner shell, the line's left end or the stack's bottom.",
        "adaptive" => "When frames run late, render with fewer samples, then fewer bounces, and step back up when there is time again.",
        "camera" => "Static: a fixed overview. Orbit: circles the solids. Focus: glides toward the busiest solid. Tilt: orbits while rising from 20 to 80 degrees and back, pausing half an orbit at each end. Resonance: orbits with three 20-degree rises and falls per turn.",
        "color" => "The colour gradient, or Off for plain monochrome text.",
        "color_by" => "What picks each solid's colour. Brightness: how lit the cell is. Load: the solid's metric. Spin: its spin speed and direction seen from the camera. Sound: the loudness of its own frequency band (bass on simple solids, treble on complex ones).",
        "bg_color" => "Colouring of the floor and sky, separate from the solids. Auto: by brightness when the solids are. Off: uncoloured. Match: the solids' gradient by brightness. Or a gradient of its own.",
        "bg_pattern" => "The floor's pattern. Quasicrystal is a flowing, never-repeating wave pattern.",
        "bg_lum" => "Floor brightness, 0 (no floor) to 10.",
        "bg_waves" => "Quasicrystal floor: the number of plane waves, which sets its rotational symmetry (7 = sevenfold).",
        "bg_wave_speed" => "Quasicrystal floor: how fast its waves flow.",
        "wireframe" => "Draw the edges of every solid over its shading.",
        "stats" => "A line of metric readings and frame rate along the bottom of the screen.",
        "fps_invert" => "Normally a busy machine animates faster; inverted, it slows down when busy.",
        "extras" => "Extra solids for a network adapter and/or a disk device, beside CPU, RAM, disk, network and GPU.",
        "fps_cap" => "Highest frame rate: the display's refresh rate, or a lower fixed cap.",
        "budget" => "Share of CPU time the rendering may use; the text size is chosen so it fits.",
        "overdrive" => "Ignore the CPU budget: rendering takes all the CPU time it wants (the text is sized for the whole CPU at the full frame rate, and frames render back to back). Expect full CPU load, heat and fan noise.",
        "density" => "Space heating value",
        "levels" => "How many glyphs from dark to bright: 8, 16 or 32.",
        "colors" => "Colours per gradient: 16, 32, 64 or 128. More gives smoother colour; brightness colouring then redraws more cells each frame.",
        "sound" => "A second wireframe around each solid that swells and shrinks with the sound of the default output device.",
        "halo_swing" => "How far the sound halo grows and shrinks around 1.2x the solid's size: +-10%, +-30% or +-60% of its radius.",
        "halo_color" => "Colour of the sound halo (with a colour gradient on). Under: whatever it passes over. Solid: its own solid's colour. Size: quiet halos at the gradient's start, loud ones at its end. Bass: the bass level, which also sets its brightness.",
        "halo_bands" => "On: each solid's halo follows its own frequency band, simple solids the bass and complex ones the treble. Off: every halo follows the overall level.",
        "far_ground" => "Draw the floor 8 times further out, toward the horizon. Past the normal distance it is shaded cheaply (averaged pattern, exact lighting, no sampling), so it costs little.",
        "tag_fixed" => "Leader lines keep one fixed length, following each solid as it grows and shrinks; a line only gets longer when its tag would otherwise cover another solid.",
        "auto_distance" => "The camera moves in and out so every solid stays in frame: closer when they are small or bunched up, further back as they grow or spread. Not with the focus camera.",
        "tags" => "A label with each solid's metric, joined to it by a short line.",
        "tag_color" => "Tag colour with a gradient on: complementary to the theme, a tint of the theme, or white. Always kept readable.",
        "tag_text" => "Auto: three-row block letters when the text is under 10 px tall. Line: always one line. Blocks: always block letters.",
        "shift" => "OLED burn-in protection: move the whole picture by one pixel at random intervals.",
        "shift_min_frames" => "Fewest frames between burn-in shifts.",
        "shift_max_frames" => "Most frames between burn-in shifts.",
        "shift_radius" => "How far, in pixels, the picture may drift from centre.",
        _ => "",
    }
}

/// A dialog option's shown text: long descriptions shortened to their
/// leading word ("Resonance: orbit, 3 waves ..." → "Resonance").
pub fn short_label(label: &str) -> String {
    if label.chars().count() > 10 && label.contains(' ') {
        let first = label.split_whitespace().next().unwrap_or(label);
        first.trim_end_matches([':', ',', '(']).to_string()
    } else {
        label.to_string()
    }
}
/// Quasicrystal wave counts offered in the dialog.
pub const WAVES_CHOICE_MIN: u32 = 3;
pub const WAVES_CHOICE_MAX: u32 = 12;
/// Quasicrystal flow speeds offered in the dialog: (radians/s, shown text).
pub const WAVE_SPEEDS: [(&str, &str); 5] =
    [("0", "Still"), ("0.3", "Slow"), ("0.6", "Medium"), ("1.2", "Fast"), ("2.4", "Very fast")];
/// Largest burn-in drift from the centred position, pixels per axis.
pub const SHIFT_RADIUS_MAX: u8 = 25;
/// Highest rendering resolution (Industrial Boiler: 6x the characters per axis).
pub const DENSITY_MAX: u8 = 6;

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
    l.id()
}

/// `s` with its first letter in capitals (shown option names; the INI
/// values stay lowercase).
fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
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
    /// Shown in the Appearance group (every look setting is; so are a few
    /// that always apply).
    pub appearance: bool,
    /// Shown in this secondary dialog instead of the main one.
    pub panel: Option<Panel>,
}

/// Every drop-down in the configuration dialog, in display order.
pub fn choices() -> Vec<Choice> {
    let opt = |v: &str, t: &str| (v.to_string(), t.to_string());
    let on_off = || vec![opt("on", "On"), opt("off", "Off")];
    let mut colors = vec![opt("off", "Off (monochrome)")];
    colors.extend(color::PRESETS.iter().map(|(id, _, _)| opt(id, &capitalized(id))));
    let mut bg_colors = vec![
        opt("auto", "Auto (by brightness when the solids are)"),
        opt("off", "Off (uncoloured)"),
        opt("match", "Match (the solids' gradient, by brightness)"),
    ];
    bg_colors.extend(color::PRESETS.iter().map(|(id, _, _)| opt(id, &capitalized(id))));
    let patterns = Pattern::ALL.iter().map(|p| opt(p.id(), &capitalized(p.id()))).collect();
    let lums = (0..=10).map(|l| opt(&l.to_string(), &if l == 0 { "0 (no floor)".into() } else { l.to_string() })).collect();
    let waves = (WAVES_CHOICE_MIN..=WAVES_CHOICE_MAX).map(|n| opt(&n.to_string(), &format!("{n}-fold"))).collect();
    let speeds = WAVE_SPEEDS.iter().map(|&(v, t)| opt(v, t)).collect();
    let mut caps = vec![opt("display", "Display refresh rate")];
    caps.extend([30, 60, 75, 90, 120, 144, 165, 240].map(|f| opt(&f.to_string(), &format!("{f} fps"))));
    let budgets = [5, 10, 15, 20, 25, 30, 40, 50].map(|b| opt(&b.to_string(), &format!("{b}%"))).to_vec();
    let look = |key, label, options| Choice { key, label, options, look: true, appearance: true, panel: None };
    // shown with the look, but kept as set (not randomised) every run
    let styled = |key, label, options| Choice { key, label, options, look: false, appearance: true, panel: None };
    let always = |key, label, options| Choice { key, label, options, look: false, appearance: false, panel: None };
    // sliders shown in the Tuning dialog: part of the look, or always applied
    let tuning_look = |key, label, options| Choice { key, label, options, look: true, appearance: false, panel: Some(Panel::Tuning) };
    let tuning_styled =
        |key, label, options| Choice { key, label, options, look: false, appearance: false, panel: Some(Panel::Tuning) };
    let in_panel =
        |panel, key, label, options| Choice { key, label, options, look: false, appearance: false, panel: Some(panel) };
    vec![
        look("layout", "Layout", Layout::ALL.iter().map(|l| opt(l.id(), &capitalized(l.id()))).collect()),
        in_panel(
            Panel::Layout,
            "ordering",
            "Ordering",
            vec![
                opt("listed", "As listed (metric order)"),
                opt("simple", "Simple to complex"),
                opt("complex", "Complex to simple"),
                opt("random", "Random"),
            ],
        ),
        in_panel(Panel::Layout, "orbit_center", "Orbit centre", OrbitCenter::ALL.iter().map(|c| opt(c.id(), c.label())).collect()),
        in_panel(Panel::Rendering, "adaptive", "Adaptive quality", on_off()),
        look(
            "camera",
            "Camera",
            vec![
                opt("static", "Static"),
                opt("orbit", "Orbit"),
                opt("focus", "Focus on busiest"),
                opt("tilt", "Tilt: orbit, rising 20-80 degrees and back"),
                opt("resonance", "Resonance: orbit, 3 waves of 20 degrees per turn"),
            ],
        ),
        look("color", "Colour", colors),
        look(
            "color_by",
            "Colour source (solids)",
            vec![
                opt("lum", "Brightness"),
                opt("load", "Load per solid"),
                opt("spin", "Spin speed and direction"),
                opt("sound", "Sound: frequency band per solid"),
            ],
        ),
        look("bg_color", "Background colour", bg_colors),
        look("bg_pattern", "Background pattern", patterns),
        tuning_look("bg_lum", "Background luminosity", lums),
        tuning_look("bg_waves", "Quasicrystal waves", waves),
        tuning_look("bg_wave_speed", "Quasicrystal flow", speeds),
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
        always("overdrive", "Overdrive", on_off()),
        always(
            "density",
            "Rendering resolution",
            vec![
                opt("1", "Standard"),
                opt("2", "Double"),
                opt("3", "Triple"),
                opt("4", "Quadruple"),
                opt("5", "Quintuple"),
                // a no-break space keeps both words in the drop-down
                opt("6", "Industrial\u{a0}Boiler"),
            ],
        ),
        always(
            "levels",
            "Luminosity levels",
            vec![opt("8", "8"), opt("16", "16"), opt("32", "32")],
        ),
        always(
            "colors",
            "Palette size",
            color::PALETTE_SIZES.iter().map(|n| opt(&n.to_string(), &format!("{n} colours"))).collect(),
        ),
        styled("sound", "Sound halo", on_off()),
        tuning_styled(
            "halo_swing",
            "Halo size swing",
            HALO_SWING_PCT.iter().map(|p| opt(&p.to_string(), &format!("\u{b1}{p}%"))).collect(),
        ),
        styled("halo_bands", "Halo bounce by frequency", on_off()),
        styled(
            "halo_color",
            "Halo colour",
            vec![
                opt("under", "Under (what it passes over)"),
                opt("solid", "Solid (its own solid's colour)"),
                opt("size", "Size (quiet to loud along the gradient)"),
                opt("bass", "Bass (the bass level)"),
            ],
        ),
        always("auto_distance", "Dynamic camera distance", on_off()),
        styled("far_ground", "Extended floor distance", on_off()),
        always("tag_fixed", "Fixed tag length", on_off()),
        styled("tags", "Tags", on_off()),
        styled(
            "tag_color",
            "Tag colour",
            crate::color::TagColor::CHOICES.iter().map(|c| opt(&c.id(), c.describe())).collect(),
        ),
        styled(
            "tag_text",
            "Tag style",
            vec![
                opt("auto", "Auto (block letters under 10 px)"),
                opt("line", "Line (always one line)"),
                opt("blocks", "Blocks (always 3-line block letters)"),
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
    /// The rendering CPU budget, share of wall time: the slider's, or all
    /// of it with overdrive on.
    pub fn budget(&self) -> f32 {
        if self.overdrive { 1.0 } else { self.budget_pct as f32 / 100.0 }
    }

    /// `(key, value)` pairs as written to the INI file.
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut v = self.look_pairs();
        // Tuning: every key, empty when not set (so saving clears it).
        for t in &TUNABLES {
            let set = self.tuning.iter().find(|(k, _)| *k == t.key).map(|&(_, x)| t.show(x));
            v.push((t.ini, set.unwrap_or_default()));
        }
        v
    }

    fn look_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("random", on_off(self.random)),
            ("layout", layout_id(self.layout).into()),
            ("camera", self.camera.id().into()),
            ("color", self.color.clone()),
            ("color_by", self.color_by.id().into()),
            ("bg_color", self.bg_color.clone()),
            ("bg_pattern", self.bg_pattern.id().into()),
            ("bg_lum", self.bg_lum.to_string()),
            ("bg_waves", self.bg_waves.to_string()),
            ("bg_wave_speed", self.bg_wave_speed.to_string()),
            ("wireframe", on_off(self.wireframe)),
            ("stats", on_off(self.stats)),
            ("fps_invert", on_off(self.fps_invert)),
            ("extras", extras_id(self.extras).into()),
            ("fps_cap", self.fps_cap.map_or_else(|| "display".to_string(), |f| f.to_string())),
            ("budget", self.budget_pct.to_string()),
            ("overdrive", on_off(self.overdrive)),
            ("density", self.density.to_string()),
            ("levels", levels_id(self.levels).into()),
            ("colors", self.colors.to_string()),
            ("sound", on_off(self.sound)),
            ("halo_swing", self.halo_swing.to_string()),
            ("halo_bands", on_off(self.halo_bands)),
            ("halo_color", self.halo_color.id().into()),
            ("ordering", self.ordering.id().into()),
            ("orbit_center", self.orbit_center.id().into()),
            ("adaptive", on_off(self.adaptive)),
            ("auto_distance", on_off(self.auto_distance)),
            ("far_ground", on_off(self.far_ground)),
            ("tag_fixed", on_off(self.tag_fixed)),
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
            bg_color: val("bg_color")
                .filter(|v| ["auto", "off", "match"].contains(&v.as_str()) || color::PRESETS.iter().any(|(id, _, _)| id == v))
                .unwrap_or(d.bg_color.clone()),
            bg_pattern: val("bg_pattern")
                .and_then(|v| Pattern::ALL.into_iter().find(|p| p.id() == v))
                .unwrap_or(d.bg_pattern),
            bg_lum: num("bg_lum").filter(|l| *l <= 10).map_or(d.bg_lum, |l| l as u8),
            bg_waves: num("bg_waves").filter(|n| (WAVES_CHOICE_MIN..=WAVES_CHOICE_MAX).contains(n)).unwrap_or(d.bg_waves),
            bg_wave_speed: val("bg_wave_speed")
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite() && (-20.0..=20.0).contains(v))
                .unwrap_or(d.bg_wave_speed),
            wireframe: flag("wireframe", d.wireframe),
            stats: flag("stats", d.stats),
            fps_invert: flag("fps_invert", d.fps_invert),
            extras: val("extras").and_then(|v| parse_extras(&v)).unwrap_or(d.extras),
            fps_cap: match val("fps_cap").as_deref() {
                None | Some("display") => None,
                Some(v) => v.parse::<u32>().ok().filter(|f| (30..=240).contains(f)).or(d.fps_cap),
            },
            budget_pct: num("budget").filter(|b| (1..=100).contains(b)).map_or(d.budget_pct, |b| b as u8),
            overdrive: flag("overdrive", d.overdrive),
            density: num("density").filter(|v| (1..=DENSITY_MAX as u32).contains(v)).map_or(d.density, |v| v as u8),
            levels: val("levels").and_then(|v| parse_levels(&v)).unwrap_or(d.levels),
            colors: num("colors").map(|n| n as usize).filter(|n| color::PALETTE_SIZES.contains(n)).unwrap_or(d.colors),
            sound: flag("sound", d.sound),
            halo_swing: num("halo_swing")
                .and_then(|p| HALO_SWING_PCT.into_iter().find(|&q| q as u32 == p))
                .or_else(|| val("halo_wide").and_then(|v| parse_on_off(&v)).map(|w| if w { 30 } else { 10 }))
                .unwrap_or(d.halo_swing),
            tuning: TUNABLES
                .iter()
                .filter_map(|t| {
                    let v = val(t.ini)?.parse::<f32>().ok().filter(|v| v.is_finite())?;
                    Some((t.key, t.snap(v)))
                })
                .collect(),
            halo_bands: flag("halo_bands", d.halo_bands),
            halo_color: val("halo_color").and_then(|v| color::HaloColor::parse(&v)).unwrap_or(d.halo_color),
            ordering: val("ordering").and_then(|v| v.parse().ok()).unwrap_or(d.ordering),
            orbit_center: val("orbit_center").and_then(|v| v.parse().ok()).unwrap_or(d.orbit_center),
            adaptive: flag("adaptive", d.adaptive),
            auto_distance: flag("auto_distance", d.auto_distance),
            far_ground: flag("far_ground", d.far_ground),
            tag_fixed: flag("tag_fixed", d.tag_fixed),
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
        c.bg_color = self.bg_color.clone();
        c.bg_pattern = self.bg_pattern;
        c.bg_lum = self.bg_lum as f32;
        c.bg_waves = self.bg_waves;
        c.bg_wave_speed = self.bg_wave_speed;
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

    #[test]
    fn overdrive_takes_the_whole_cpu() {
        let d = Display { width: 2560, height: 1440, refresh: 60 };
        let s = ScrSettings { random: false, budget_pct: 10, ..ScrSettings::default() };
        let od = ScrSettings { overdrive: true, ..s.clone() };
        assert_eq!((s.budget(), od.budget()), (0.10, 1.0));
        let (p, q) = (plan(&d, 4, &s).unwrap(), plan(&d, 4, &od).unwrap());
        assert_eq!(q.budget, 1.0);
        // the same speed test sizes the text for all of the CPU: smaller text, more cells
        let (px_full, ..) = font_height(&d, q.config.fps_max, p.sec_per_cell, 1.0);
        let (px_ten, ..) = font_height(&d, p.config.fps_max, p.sec_per_cell, 0.10);
        assert!(px_full <= px_ten);
        assert!(preview_needs_restart(&s, &od), "overdrive sets the text size");
        assert!(preview_settings(&preview_text(&od)).overdrive);
    }

    #[test]
    fn preview_file_round_trips_and_only_text_size_needs_a_restart() {
        let s = ScrSettings {
            random: false,
            camera: CameraMode::Tilt,
            tuning: vec![("orbit_period", 40.0)],
            fps_cap: Some(30),
            ..ScrSettings::default()
        };
        assert_eq!(preview_settings(&preview_text(&s)), s);
        assert_eq!(preview_settings(""), ScrSettings::default());
        // look and every-run changes apply live
        let live = ScrSettings { camera: CameraMode::Orbit, sound: false, colors: 64, random: true, ..s.clone() };
        assert!(!preview_needs_restart(&s, &live));
        assert_eq!(preview_live(&s, &live), live);
        // the text size settings need a restart, and stay as they were until then
        for edited in [
            ScrSettings { budget_pct: 40, ..s.clone() },
            ScrSettings { density: 1, ..s.clone() },
            ScrSettings { fps_cap: None, ..s.clone() },
        ] {
            assert!(preview_needs_restart(&s, &edited));
            let applied = preview_live(&s, &edited);
            assert_eq!((applied.budget_pct, applied.density, applied.fps_cap), (s.budget_pct, s.density, s.fps_cap));
        }
        for k in PREVIEW_RESTART_KEYS {
            assert!(choices().iter().any(|c| c.key == k), "{k} is a dialog setting");
        }
        // the cheap re-plan is the plan's own config
        let d = Display { width: 1920, height: 1080, refresh: 60 };
        let p = plan(&d, 9, &s).unwrap();
        assert_eq!(plan_config(&d, 9, &s).to_toml(), p.config.to_toml());
    }

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
        assert_eq!(apply_density(&WQHD, FONT_MIN, 2).0, 4);
        assert_eq!(apply_density(&WQHD, 18, 6).0, 3, "Industrial Boiler: a sixth of the height");
        assert_eq!(apply_density(&WQHD, 5, 6).0, FONT_FLOOR, "never below the floor");
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
            color_by: ColorBy::Spin,
            bg_color: "ice".into(),
            bg_pattern: Pattern::Hex,
            bg_lum: 0,
            bg_waves: 5,
            bg_wave_speed: 0.0,
            wireframe: true,
            stats: false,
            fps_invert: true,
            extras: Extras::Both,
            fps_cap: Some(60),
            budget_pct: 25,
            overdrive: true,
            density: 1,
            levels: Levels::L32,
            colors: 64,
            sound: false,
            halo_swing: 60,
            tuning: vec![("orbit_period", 45.0), ("max_spin", 2.5)],
            halo_bands: false,
            halo_color: crate::color::HaloColor::Size,
            ordering: Ordering::Complex,
            orbit_center: OrbitCenter::Spin,
            adaptive: false,
            auto_distance: true,
            far_ground: true,
            tag_fixed: true,
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
            ["layout", "camera", "color", "color_by", "bg_color", "bg_pattern", "bg_lum", "bg_waves", "bg_wave_speed", "wireframe", "stats", "fps_invert", "extras"]
        );
        for c in choices() {
            assert!(keys.contains(&c.key), "{} is a settings key", c.key);
            // (old INI files used "random" per setting; the ordering's random is its own, newer choice)
            assert!(c.key == "ordering" || c.options.iter().all(|(v, _)| v != "random"), "{}: no per-setting random", c.key);
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
    fn tuning_applies_every_run_and_snaps_to_its_steps() {
        for random in [true, false] {
            let s = ScrSettings { random, tuning: vec![("orbit_period", 45.0), ("bg_scale", 2.0), ("exposure", 1.3)], ..ScrSettings::default() };
            for seed in 0..5 {
                let c = plan(&WQHD, seed, &s).unwrap().config;
                assert_eq!((c.orbit_period, c.bg_scale, c.exposure), (45.0, 2.0, 1.3), "random {random} seed {seed}");
            }
        }
        // out of range or off-step values from the INI snap to the slider
        let s = ScrSettings::from_lookup(lookup(&[("tune_orbit_period", "47"), ("tune_max_spin", "99"), ("tune_bg_sway", "x")]));
        assert_eq!(s.tuning, vec![("orbit_period", 45.0), ("max_spin", 8.0)]);
        for t in TUNABLES {
            assert_eq!(t.value_at(0), t.min, "{}", t.key);
            assert!((t.value_at(t.steps()) - t.max).abs() < 1e-4, "{}", t.key);
            let d = get_tunable(&Config::default(), t.key);
            assert!(d.is_finite() && (t.min..=t.max).contains(&d), "{}: default {d} inside the slider", t.key);
            let mut c = Config::default();
            set_tunable(&mut c, t.key, t.max);
            assert_eq!(get_tunable(&c, t.key), t.max, "{}", t.key);
        }
        // the old on/off halo key still reads: on = 30%, off = 10%
        assert_eq!(ScrSettings::from_lookup(lookup(&[("halo_wide", "on")])).halo_swing, 30);
        assert_eq!(ScrSettings::from_lookup(lookup(&[("halo_wide", "off")])).halo_swing, 10);
        assert_eq!(ScrSettings::from_lookup(lookup(&[("halo_swing", "60"), ("halo_wide", "off")])).halo_swing, 60);
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
