//! Colour benchmark: per-frame cost of colouring at screensaver size, by
//! palette size and colour source, over an animated 16-solid scene.
//!
//! For each case it reports the overlay compose time (tone map, glyphs and
//! colours), a simulation of the screensaver's draw stage (skip unchanged
//! cells, else fetch the glyph tile tinted in the cell's colour from a
//! bounded cache, tinting on a miss, and copy it out) and the colours used.
//!
//! `cargo run --release --example color_bench [frames]`

use std::collections::HashSet;
use std::hint::black_box;
use std::time::Instant;

use platomon::ascii::Ramp;
use platomon::color::{ColorBy, ColorMap, Gradient};
use platomon::metrics::Channel;
use platomon::overlay::{OverlayOptions, Overlays};
use platomon::render::{FrameView, Projection, RenderSettings, Renderer, TraceSettings};
use platomon::scene::ground::{GroundParams, Pattern};
use platomon::scene::sdf::SolidKind;
use platomon::scene::{AnimParams, CameraParams, Scene, SlotLayout};

const W: usize = 640;
const H: usize = 240;
/// Glyph tile at density 2 on a 2560×1440 screen (4 × 6 px).
const TILE: usize = 24;

struct Frame {
    radiance: Vec<f32>,
    depth: Vec<f32>,
    ids: Vec<u8>,
    proj: Projection,
    scene: Scene,
    load: Vec<f32>,
    spin: Vec<f32>,
    sound: Vec<f32>,
}

impl Frame {
    fn view(&self) -> FrameView<'_> {
        FrameView { radiance: &self.radiance, depth: &self.depth, ids: &self.ids, proj: self.proj }
    }
}

/// An animated scene: loads drift, spins follow them, and a synthetic
/// "music" level moves each solid's band at its own rate.
fn frames(n: usize, count: usize) -> Vec<Frame> {
    let anim = AnimParams::default();
    let slots: Vec<SlotLayout> =
        (0..n).map(|i| SlotLayout { solid: SolidKind::ALL[i % SolidKind::ALL.len()], ring: None }).collect();
    let bg = GroundParams { lum: 5.0, pattern: Pattern::Checker, contrast: 0.6, scale: 1.5, ..GroundParams::default() };
    let mut s = Scene::new(&slots, &anim, &CameraParams::default()).with_ground(&bg);
    let rs = RenderSettings { spp: 1, trace: TraceSettings { bounces: 1, ..TraceSettings::default() }, ..RenderSettings::default() };
    let mut r = Renderer::new();
    let mut out = Vec::with_capacity(count);
    for f in 0..count {
        let t = f as f32 / 30.0;
        let ch: Vec<Channel> = (0..n)
            .map(|i| {
                let l = 0.5 + 0.45 * (t * (0.7 + 0.13 * i as f32) + i as f32).sin();
                Channel { available: true, level: l, spin_y: l, spin_x: 0.3 * l, activity: l }
            })
            .collect();
        s.update(&ch, &anim, 1.0 / 30.0);
        r.render(&s, W, H, &rs);
        let v = r.view().unwrap();
        let mut spin = Vec::new();
        s.spin_values(&mut spin);
        let sound = (0..n).map(|i| (0.5 + 0.5 * (t * (2.0 + 0.9 * i as f32)).sin()).powi(2)).collect();
        out.push(Frame {
            radiance: v.radiance.to_vec(),
            depth: v.depth.to_vec(),
            ids: v.ids.to_vec(),
            proj: v.proj,
            scene: s.clone(),
            load: ch.iter().map(|c| c.level).collect(),
            spin,
            sound,
        });
    }
    out
}

/// Fake ink coverage for glyph `u` (the screensaver renders it with GDI).
fn coverage(u: u16) -> Vec<u32> {
    (0..TILE as u32).map(|k| ((u as u32 * 37 + k * 11) % 256) * 0x01_01_01).collect()
}

/// The draw stage: cells whose glyph and colour are unchanged are skipped;
/// the rest copy their tinted tile into the frame buffer.
struct Draw {
    tiles: platomon::tiles::TileCache,
    shown: Vec<Option<(u16, u32)>>,
    pixels: Vec<u32>,
}

impl Draw {
    fn new() -> Self {
        Self { tiles: platomon::tiles::TileCache::new(TILE), shown: vec![None; W * H], pixels: vec![0; W * H * TILE] }
    }

    fn frame(&mut self, chars: &[char], colors: Option<&[u32]>) -> usize {
        let mut drawn = 0;
        for (i, &c) in chars.iter().enumerate() {
            let u = c as u16;
            let rgb = colors.map_or(0xC0C0C0, |v| if v[i] == platomon::color::NONE { 0xC0C0C0 } else { v[i] });
            if self.shown[i] == Some((u, rgb)) {
                continue;
            }
            self.shown[i] = Some((u, rgb));
            let t = self.tiles.tile(u, rgb, coverage);
            self.pixels[i * TILE..(i + 1) * TILE].copy_from_slice(t);
            drawn += 1;
        }
        drawn
    }
}

struct Case {
    name: String,
    cm: Option<ColorMap>,
    by: ColorBy,
}

fn cases() -> Vec<Case> {
    let g = |s: &str| Gradient::parse(s).ok();
    let mut v = vec![Case { name: "monochrome".into(), cm: None, by: ColorBy::Lum }];
    for by in ColorBy::ALL {
        for n in platomon::color::PALETTE_SIZES {
            let grad = g(platomon::color::default_for(by));
            v.push(Case { name: format!("{} {n}", by.id()), cm: ColorMap::new(grad.clone(), by, n, platomon::color::bg_gradient("auto", grad.as_ref(), by).unwrap()), by });
        }
    }
    for (name, by, bg) in [("load 16 + bg ice", ColorBy::Load, "ice"), ("spin 16 + bg match", ColorBy::Spin, "match"), ("lum 16, bg off", ColorBy::Lum, "off")] {
        let grad = g(platomon::color::default_for(by));
        v.push(Case { name: name.into(), cm: ColorMap::new(grad.clone(), by, 16, platomon::color::bg_gradient(bg, grad.as_ref(), by).unwrap()), by });
    }
    v
}

fn main() {
    let count: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(90);
    let fr = frames(16, count);
    let ramp = Ramp::default();
    println!("--- 16 solids, {W}x{H} cells, {count} animated frames, best of 3 rounds (ms per frame)");
    println!("{:<20} {:>8} {:>8} {:>8} {:>9} {:>8} {:>7}", "case", "compose", "draw", "total", "colours", "cells", "misses");
    let all = cases();
    let mut best: Vec<(f64, f64, usize, f64, f64)> = vec![(f64::MAX, f64::MAX, 0, 0.0, 0.0); all.len()];
    for _ in 0..3 {
        for (case, b) in all.iter().zip(&mut best) {
            let mut ov = Overlays::new(&fr[0].scene);
            let mut draw = Draw::new();
            let (mut tc, mut td, mut drawn) = (0.0f64, 0.0f64, 0usize);
            let mut colours: HashSet<u32> = HashSet::new();
            for f in &fr {
                let levels = match case.by {
                    ColorBy::Spin => &f.spin,
                    ColorBy::Sound => &f.sound,
                    _ => &f.load,
                };
                let opt = OverlayOptions {
                    wireframe: false,
                    legend: None,
                    labels: &[],
                    char_aspect: 0.5,
                    ramp: &ramp,
                    exposure: 1.0,
                    color: case.cm.as_ref(),
                    levels,
                    halo: None,
                    halo_factors: &[],
                    halo_color: Default::default(),
                    tags: false,
                    tag_labels: &[],
                    tag_ink: platomon::color::NONE,
                    tag_scale: 0,
                    tag_fixed: false,
                };
                let view = f.view();
                let t0 = Instant::now();
                let cells = ov.compose(&view, W, H, &f.scene, &opt);
                let t1 = Instant::now();
                drawn += black_box(draw.frame(cells.chars, cells.colors));
                let t2 = Instant::now();
                tc += (t1 - t0).as_secs_f64();
                td += (t2 - t1).as_secs_f64();
                if let Some(c) = cells.colors {
                    colours.extend(c.iter().copied().filter(|&c| c != platomon::color::NONE));
                }
            }
            let n = fr.len() as f64;
            if tc + td < b.0 + b.1 {
                *b = (tc * 1e3 / n, td * 1e3 / n, colours.len(), drawn as f64 / n, draw.tiles.misses as f64 / n);
            }
        }
    }
    for (case, &(c, d, k, cells, miss)) in all.iter().zip(&best) {
        println!("{:<20} {c:8.3} {d:8.3} {:8.3} {k:9} {cells:8.0} {miss:7.1}", case.name, c + d);
    }
}
