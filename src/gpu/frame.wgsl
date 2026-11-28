// Space heater: one frame of the path tracer on the GPU, one invocation per
// character cell, straight to the text stream the CPU draws.
//
// A port of src/render (trace.rs, march.rs) and the scene's distance
// functions and floor patterns; keep it in step with them. Written to
// Vulkan 1.1 / SPIR-V 1.3 core with no extensions and modest limits
// (8 × 8 workgroups, storage buffers only), so a GCN 1.0 card such as a
// Radeon HD 7850 runs it.
//
// World: z up, the floor is the plane z = 0.

struct Frame {
    cam_pos: vec4<f32>,
    cam_fwd: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    // k, half width, half height, cell aspect
    proj: vec4<f32>,
    // max_dist, detail_dist, pixel_angle, scene extent
    trace: vec4<f32>,
    // light centre xyz, radius
    light: vec4<f32>,
    // emission, sky, background, exposure
    shade: vec4<f32>,
    // scene centroid xyz, slab top
    centroid: vec4<f32>,
    // floor: base albedo, contrast, 1 / tile size, fade rate
    ground0: vec4<f32>,
    // floor: pivot (floor frame) x, y, sin and cos of -angle
    ground1: vec4<f32>,
    // floor: offset x, y (floor frame), dot size, dot spacing
    ground2: vec4<f32>,
    // floor: dot variability, unused
    ground3: vec4<f32>,
    // width, height, samples per cell, bounces
    dims: vec4<u32>,
    // max steps, solids, frame seed, ramp length
    misc: vec4<u32>,
    // floor: pattern, enabled, invert half, unused
    groundi: vec4<u32>,
    // discrete pixels: 0 off (text stream), 2 traced, 3 the overlay only (a
    // frame the CPU drew), 4 the halo redrawn over the last traced picture,
    // all straight to the screen;
    // background palette size, solids' palette size,
    // flags (1 = coloured, 2 = per-solid table, 4 = R and B swapped)
    pix: vec4<u32>,
    // discrete pixels: ink (0xRRGGBB), output width, height, unused
    pix2: vec4<u32>,
    // discrete pixels: burn-in shift x, y (pixels), unused
    pix3: vec4<i32>,
    // discrete pixels: two overlay rectangles (x, y, width, height; width 0
    // = none) and where each one's pixels start in `overlay`
    ov0: vec4<i32>,
    ov1: vec4<i32>,
    ov_off: vec4<u32>,
    // the animated and parametric floors' parameters (Ground::shader_pattern)
    pat: vec4<f32>,
    // the sound halo over discrete pixels: brightness, half the line's
    // width (pixels), solids with a halo, on (overlay.rs: halo_lines)
    halo: vec4<f32>,
}

struct Solid {
    // centre xyz, circumradius
    centre: vec4<f32>,
    // 1 / circumradius, albedo, curvature / 10 (-1..1; 0 = flat), unused
    params: vec4<f32>,
    // world → model rotation, by columns
    rot0: vec4<f32>,
    rot1: vec4<f32>,
    rot2: vec4<f32>,
    // first plane, plane count, centrally symmetric, unused
    planes: vec4<u32>,
}

@group(0) @binding(0) var<storage, read> frame: Frame;
@group(0) @binding(1) var<storage, read> solids: array<Solid>;
// face planes: normal xyz, offset (circumradius 1); a curved solid's
// planes are followed by one dish per plane (centre distance, radius, -, -)
@group(0) @binding(2) var<storage, read> planes: array<vec4<f32>>;
// per cell: glyph level | solid id << 8 | luminance (16 bits) << 16, then depth bits
@group(0) @binding(3) var<storage, read_write> cells: array<vec2<u32>>;
// discrete pixels: background palette, solids' palette, per-solid table
// (solid × ramp level), 0xRRGGBB or NONE (src/overlay.rs: PixelPalette)
@group(0) @binding(4) var<storage, read> palette: array<u32>;
// discrete pixels, ready for the screen: 0xAARRGGBB per pixel
@group(0) @binding(5) var<storage, read_write> pixels: array<u32>;
// what the CPU drew over them (0x00RRGGBB, the overlay rectangles' pixels)
@group(0) @binding(6) var<storage, read> overlay: array<u32>;
// the floor pattern's items: wave directions, raindrops or ocean waves
// (Ground::shader_pattern)
@group(0) @binding(7) var<storage, read> waves: array<vec4<f32>>;
// the halo's lines: per solid three vec4s (bounding box; first line, line
// count, colour or NONE for the colour under it, solid as u32 bits; depth
// slack), then per line two (from x, y, to x, y in picture pixels; depth
// at each end)
@group(0) @binding(8) var<storage, read> halo_lines: array<vec4<f32>>;
// the last traced discrete pixels, before the halo: colour (0xRRGGBB, the
// solid hit in the top byte), depth bits; halo-only frames redraw from it
@group(0) @binding(9) var<storage, read_write> traced: array<vec2<u32>>;

const INF: f32 = 3.4e38;
const TAU: f32 = 6.2831853;
const GROUND: u32 = 255u;
const NO_HIT: u32 = 254u;
const SPHERE_SWITCH: f32 = 0.25;
const SCENE_MARGIN: f32 = 2.0;
const RAY_OFFSET: f32 = 2e-3;
const AMBIENT_OCCLUSION: f32 = 0.5;
const GRAZING_MIN: f32 = 0.02;
const RIM: f32 = 0.15;

// floor patterns the GPU knows (src/gpu/mod.rs: PATTERNS)
const P_SOLID: u32 = 0u;
const P_CHECKER: u32 = 1u;
const P_DIAMOND: u32 = 2u;
const P_BRICK: u32 = 3u;
const P_TRIANGLES: u32 = 4u;
const P_HEX: u32 = 5u;
const P_POLKA: u32 = 6u;
const P_RADIAL: u32 = 7u;
const P_QUASICRYSTAL: u32 = 8u;
const P_RAIN: u32 = 9u;
const P_OCEAN: u32 = 10u;
const P_JULIA: u32 = 11u;
const P_CLOUDS: u32 = 12u;
const P_CARPET: u32 = 13u;
const RAIN_CELL: f32 = 12.0;
const RAIN_TRAIN: f32 = 3.0;
const JULIA_SCALE: f32 = 4.0;
const NONE: u32 = 0xffffffffu;

// ------------------------------------------------------------------ random numbers

var<private> rng: u32;

fn hash(v: u32) -> u32 {
    var s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rnd() -> f32 {
    rng = hash(rng);
    return f32(rng >> 8u) * (1.0 / 16777216.0);
}

// ------------------------------------------------------------------ solids

fn sdf_local(i: u32, dc: vec3<f32>) -> f32 {
    let s = solids[i];
    let q = dc * s.params.x;
    let p = s.rot0.xyz * q.x + s.rot1.xyz * q.y + s.rot2.xyz * q.z;
    var m = -INF;
    let first = s.planes.x;
    let n = s.planes.y;
    let sym = s.planes.z != 0u;
    if sym {
        for (var k = 0u; k < n; k++) {
            let pl = planes[first + k];
            m = max(m, abs(dot(p, pl.xyz)) - pl.w);
        }
    } else {
        for (var k = 0u; k < n; k++) {
            let pl = planes[first + k];
            m = max(m, dot(p, pl.xyz) - pl.w);
        }
    }
    // curvature (src/scene/sdf.rs: Polyhedron::eval): bulge toward the
    // circumsphere, or carve each face's dish
    let c = s.params.z;
    if c > 0.0 {
        m = m + c * (length(p) - 1.0 - m);
    } else if c < 0.0 {
        let pp = dot(p, p);
        for (var k = 0u; k < n; k++) {
            var x = dot(p, planes[first + k].xyz);
            if sym {
                x = abs(x);
            }
            let dish = planes[first + n + k];
            let dx = x - dish.x;
            m = max(m, dish.y - sqrt(max(pp - x * x, 0.0) + dx * dx));
        }
    }
    return m * s.centre.w;
}

struct Near {
    d: f32,
    id: u32,
}

fn solids_sdf(p: vec3<f32>, mask: u32) -> Near {
    var best = INF;
    var id = 0u;
    for (var i = 0u; i < frame.misc.y; i++) {
        if (mask & (1u << i)) == 0u {
            continue;
        }
        let dc = p - solids[i].centre.xyz;
        let ds = length(dc) - solids[i].centre.w;
        if ds >= best {
            continue;
        }
        var d = ds;
        if ds <= SPHERE_SWITCH * solids[i].centre.w {
            d = sdf_local(i, dc);
        }
        if d < best {
            best = d;
            id = i;
        }
    }
    return Near(best, id);
}

struct Hit {
    t: f32,
    id: u32,
    ok: bool,
}

const MISS: Hit = Hit(INF, 0u, false);

// Solids only, over tmin..tmax: the height band holding them, then just the
// solids whose bounding spheres the ray meets, where it meets them.
fn march_solids(ro: vec3<f32>, rd: vec3<f32>, tmin: f32, tmax: f32) -> Hit {
    var t = tmin;
    var end = tmax;
    let top = frame.centroid.w;
    if abs(rd.z) < 1e-8 {
        if ro.z < 0.0 || ro.z > top {
            return MISS;
        }
    } else {
        let inv = 1.0 / rd.z;
        let a = (0.0 - ro.z) * inv;
        let b = (top - ro.z) * inv;
        t = max(t, min(a, b));
        end = min(end, max(a, b));
    }
    if t > end {
        return MISS;
    }
    let pad = 1e-3 * min(end, 1e4) + 1e-3;
    var mask = 0u;
    var lo = INF;
    var hi = -INF;
    for (var i = 0u; i < frame.misc.y; i++) {
        let r = solids[i].centre.w + pad;
        let oc = ro - solids[i].centre.xyz;
        let b = dot(oc, rd);
        let disc = b * b - (dot(oc, oc) - r * r);
        if disc < 0.0 {
            continue;
        }
        let q = sqrt(disc);
        let t0 = -b - q;
        let t1 = -b + q;
        if t1 < t || t0 > end {
            continue;
        }
        mask |= 1u << i;
        lo = min(lo, t0);
        hi = max(hi, t1);
    }
    if mask == 0u {
        return MISS;
    }
    t = max(lo, t);
    end = min(hi, end);
    for (var s = 0u; s < frame.misc.x; s++) {
        let n = solids_sdf(ro + rd * t, mask);
        if n.d < max(1e-3 * t, 1e-4) {
            return Hit(t, n.id, true);
        }
        t += n.d;
        if t > end {
            return MISS;
        }
    }
    return MISS;
}

fn floor_radius(p: vec3<f32>) -> f32 {
    return length(p.xy - frame.centroid.xy);
}

// Nearest hit with anything within the floor's reach.
fn march(ro: vec3<f32>, rd: vec3<f32>) -> Hit {
    var t_plane = INF;
    if frame.groundi.y != 0u && rd.z < -1e-7 && ro.z > 0.0 {
        t_plane = -ro.z / rd.z;
    }
    let beyond = length(ro - frame.centroid.xyz) + frame.trace.w + SCENE_MARGIN;
    let h = march_solids(ro, rd, 0.0, min(beyond, t_plane));
    if h.ok {
        return h;
    }
    if t_plane < INF && floor_radius(ro + rd * t_plane) <= frame.trace.x {
        return Hit(t_plane, GROUND, true);
    }
    return MISS;
}

fn normal(id: u32, p: vec3<f32>) -> vec3<f32> {
    if id == GROUND {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    let dc = p - solids[id].centre.xyz;
    let h = 5e-4 * max(solids[id].centre.w, 0.1);
    let k0 = vec3<f32>(1.0, 1.0, -1.0);
    let k1 = vec3<f32>(-1.0, -1.0, -1.0);
    let k2 = vec3<f32>(-1.0, 1.0, 1.0);
    let k3 = vec3<f32>(1.0, -1.0, 1.0);
    let n = k0 * sdf_local(id, dc + k0 * h) + k1 * sdf_local(id, dc + k1 * h) + k2 * sdf_local(id, dc + k2 * h)
        + k3 * sdf_local(id, dc + k3 * h);
    let l = length(n);
    if l > 0.0 {
        return n / l;
    }
    return vec3<f32>(0.0, 0.0, 1.0);
}

// ------------------------------------------------------------------ floor patterns

fn parity(n: f32) -> f32 {
    return select(-1.0, 1.0, (i32(n) & 1) == 0);
}

fn round_away(x: f32) -> f32 {
    return sign(x) * floor(abs(x) + 0.5);
}

fn hash2(x: i32, y: i32) -> f32 {
    var h = (bitcast<u32>(x) * 0x8da6b343u) ^ (bitcast<u32>(y) * 0xd8163841u);
    h ^= h >> 13u;
    h *= 0x5bd1e995u;
    h ^= h >> 15u;
    return f32(h & 0xffffu) / 65535.0;
}

fn polka(uv: vec2<f32>, size: f32, spacing: f32, variability: f32) -> f32 {
    let row = 0.8660254;
    let g = uv / spacing;
    let r = 0.5 * size / spacing;
    let j = g.y / row;
    let i = g.x - 0.5 * j;
    let i0 = i32(floor(i));
    let j0 = i32(floor(j));
    let far = r * (1.0 + variability) + 0.71 * variability;
    var reach = 0;
    if far > 0.5 {
        reach = min(i32(ceil(far / row)), 2);
    }
    for (var dj = -reach; dj <= reach + 1; dj++) {
        for (var di = -reach; di <= reach + 1; di++) {
            let x = i0 + di;
            let y = j0 + dj;
            var c = vec2<f32>(f32(x) + 0.5 * f32(y), f32(y) * row);
            var rad = r;
            if variability > 0.0 {
                rad *= 1.0 + variability * (2.0 * hash2(x, y) - 1.0);
                let jitter = vec2<f32>(hash2(y + 7919, x - 3571), hash2(x * 3 + 11, y * 5 - 7));
                c += variability * (jitter - 0.5);
            }
            let d = g - c;
            if dot(d, d) < rad * rad {
                return 1.0;
            }
        }
    }
    return -1.0;
}

fn cos_turns(x: f32) -> f32 {
    return cos(TAU * (x - floor(x)));
}

fn quasicrystal(uv: vec2<f32>) -> f32 {
    var s = 0.0;
    for (var k = 0u; k < u32(frame.pat.y); k++) {
        s += 0.5 + 0.5 * cos_turns(dot(uv, waves[k].xy) + frame.pat.x);
    }
    let k = floor(s);
    let f = s - k;
    var v = f;
    if (i32(k) & 1) == 1 {
        v = 1.0 - f;
    }
    return 2.0 * v - 1.0;
}

fn rain(uv: vec2<f32>) -> f32 {
    let wavelength = frame.pat.x;
    var h = 0.0;
    for (var k = 0u; k < u32(frame.pat.y); k++) {
        let drop = waves[k];
        var d = uv - drop.xy;
        // nearest copy of the drop
        d -= vec2<f32>(round_away(d.x / RAIN_CELL), round_away(d.y / RAIN_CELL)) * RAIN_CELL;
        let r = length(d);
        let behind = (drop.z - r) / wavelength;
        if behind < -0.25 || behind >= RAIN_TRAIN {
            continue;
        }
        let lead = clamp(behind * 4.0 + 1.0, 0.0, 1.0);
        let tail = 1.0 - behind / RAIN_TRAIN;
        h += drop.w * lead * tail * cos_turns(behind) / (1.0 + 0.15 * r);
    }
    return clamp(2.5 * h, -1.0, 1.0);
}

fn ocean(uv: vec2<f32>) -> f32 {
    var h = 0.0;
    for (var k = 0u; k < u32(frame.pat.z); k++) {
        let w = waves[k];
        h += w.z * cos_turns(dot(uv, w.xy) + w.w);
    }
    let hh = 2.2 * h / frame.pat.x;
    let chop = frame.pat.y;
    var s = max(1.0 - 0.5 * chop, 0.3) * hh;
    if hh > 0.0 {
        s = hh + chop * hh * hh;
    }
    return clamp(s, -1.0, 1.0);
}

fn julia(uv: vec2<f32>) -> f32 {
    var x = uv.x / JULIA_SCALE;
    var y = uv.y / JULIA_SCALE;
    let c = frame.pat.xy;
    for (var n = 0u; n < u32(frame.pat.z); n++) {
        let xx = x * x;
        let yy = y * y;
        if xx + yy > 16.0 {
            // smooth escape count
            let nu = f32(n) + 1.0 - log(log(xx + yy) * 0.5) / 0.6931472;
            return cos_turns(nu / 6.0);
        }
        let nx = xx - yy + c.x;
        y = 2.0 * x * y + c.y;
        x = nx;
    }
    return -1.0;
}

fn radial(uv: vec2<f32>) -> f32 {
    let sector = floor(atan2(uv.y, uv.x) / TAU * frame.pat.x);
    return parity(floor(length(uv)) + sector);
}

fn rem3(a: i32) -> i32 {
    return ((a % 3) + 3) % 3;
}

fn div3(a: i32) -> i32 {
    var q = a / 3;
    if a % 3 != 0 && a < 0 {
        q -= 1;
    }
    return q;
}

fn carpet(uv: vec2<f32>) -> f32 {
    var u = i32(floor(uv.x * 3.0));
    var v = i32(floor(uv.y * 3.0));
    for (var k = 0u; k < u32(frame.pat.x); k++) {
        if rem3(u) == 1 && rem3(v) == 1 {
            return -1.0;
        }
        u = div3(u);
        v = div3(v);
    }
    return 1.0;
}

fn value_noise(p: vec2<f32>) -> f32 {
    let fx = floor(p.x);
    let fy = floor(p.y);
    let ix = i32(fx);
    let iy = i32(fy);
    let tx = p.x - fx;
    let ty = p.y - fy;
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let a = hash2(ix, iy) + (hash2(ix + 1, iy) - hash2(ix, iy)) * sx;
    let b = hash2(ix, iy + 1) + (hash2(ix + 1, iy + 1) - hash2(ix, iy + 1)) * sx;
    return a + (b - a) * sy;
}

fn clouds(uv0: vec2<f32>) -> f32 {
    var p = (uv0 + frame.pat.xy) / 3.0;
    var amp = 1.0;
    var sum = 0.0;
    var norm = 0.0;
    for (var o = 0u; o < u32(frame.pat.z); o++) {
        sum += amp * value_noise(p + vec2<f32>(f32(o) * 17.3));
        norm += amp;
        // turn each octave a little so the lattice doesn't show
        p = vec2<f32>(p.x * 1.6 - p.y * 1.2, p.x * 1.2 + p.y * 1.6);
        amp *= frame.pat.w;
    }
    return clamp((sum / norm - 0.5) * 3.2, -1.0, 1.0);
}

fn tone(pattern: u32, uv: vec2<f32>) -> f32 {
    let u = uv.x;
    let v = uv.y;
    switch pattern {
        case P_CHECKER: {
            return parity(floor(u) + floor(v));
        }
        case P_DIAMOND: {
            let k = 0.70710678;
            return parity(floor((u + v) * k) + floor((v - u) * k));
        }
        case P_BRICK: {
            let row = floor(v);
            let shift = select(0.0, 0.5, (i32(row) & 1) == 1);
            return parity(floor(u + shift) + row);
        }
        case P_TRIANGLES: {
            let fu = u - floor(u);
            let fv = v - floor(v);
            return parity(floor(u) + floor(v) + select(0.0, 1.0, fu > fv));
        }
        case P_HEX: {
            let size = 0.5;
            let x = (1.7320508 / 3.0 * u - v / 3.0) / size;
            let z = (2.0 / 3.0 * v) / size;
            let y = -x - z;
            var rx = round_away(x);
            let ry = round_away(y);
            var rz = round_away(z);
            let dx = abs(rx - x);
            let dy = abs(ry - y);
            let dz = abs(rz - z);
            if dx > dy && dx > dz {
                rx = -ry - rz;
            } else if dy <= dz {
                rz = -rx - ry;
            }
            let m = ((i32(rx - rz) % 3) + 3) % 3;
            if m == 0 {
                return -1.0;
            }
            if m == 1 {
                return 0.0;
            }
            return 1.0;
        }
        case P_POLKA: {
            let sp = frame.ground2.w;
            return polka(uv - vec2<f32>(0.0, 0.5 * sp), frame.ground2.z, sp, frame.ground3.x);
        }
        case P_RADIAL: {
            return radial(uv);
        }
        case P_QUASICRYSTAL: {
            return quasicrystal(uv);
        }
        case P_RAIN: {
            return rain(uv);
        }
        case P_OCEAN: {
            return ocean(uv);
        }
        case P_JULIA: {
            return julia(uv);
        }
        case P_CLOUDS: {
            return clouds(uv);
        }
        case P_CARPET: {
            return carpet(uv);
        }
        default: {
            return 0.0;
        }
    }
}

// World floor point (x, y) in the patterns' own frame (x, -y), less the pivot.
fn from_pivot(p: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(p.x, -p.y) - frame.ground1.xy;
}

fn ground_albedo(p: vec3<f32>, footprint: f32) -> f32 {
    let base = frame.ground0.x;
    let contrast = frame.ground0.y;
    let pattern = frame.groundi.x;
    if pattern == P_SOLID || contrast <= 0.0 {
        return base;
    }
    let fade = clamp(1.0 - 1.5 * footprint * frame.ground0.w, 0.0, 1.0);
    if fade <= 0.0 {
        return base;
    }
    let v = from_pivot(p);
    let s = frame.ground1.z;
    let c = frame.ground1.w;
    let d = vec2<f32>(v.x * c + v.y * s, v.y * c - v.x * s);
    let uv = (d - frame.ground2.xy) * frame.ground0.z;
    var k = tone(pattern, uv);
    if pattern == P_POLKA && frame.groundi.z != 0u && v.y * c - v.x * s < 0.0 {
        k = -k;
    }
    return clamp(base * (1.0 + contrast * fade * k), 0.0, 0.95);
}

// ------------------------------------------------------------------ light

// Orthonormal basis around unit n (Duff et al. 2017), pivoting on -y.
fn onb_t(n: vec3<f32>) -> vec3<f32> {
    let s = select(-1.0, 1.0, -n.y >= 0.0);
    let a = -1.0 / (s - n.y);
    let b = n.x * n.z * a;
    return vec3<f32>(1.0 + s * n.x * n.x * a, s * n.x, s * b);
}

fn onb_b(n: vec3<f32>) -> vec3<f32> {
    let s = select(-1.0, 1.0, -n.y >= 0.0);
    let a = -1.0 / (s - n.y);
    let b = n.x * n.z * a;
    return vec3<f32>(b, n.z, s + n.z * n.z * a);
}

fn cosine_hemisphere(n: vec3<f32>) -> vec3<f32> {
    let u1 = rnd();
    let u2 = rnd();
    let r = sqrt(u1);
    let ph = TAU * u2;
    return normalize(onb_t(n) * (r * cos(ph)) + onb_b(n) * (r * sin(ph)) + n * sqrt(max(1.0 - u1, 0.0)));
}

// Cosine-weighted bounce off the floor (normal +z).
fn ground_bounce() -> vec3<f32> {
    let u1 = rnd();
    let u2 = rnd();
    let r = sqrt(u1);
    let ph = TAU * u2;
    return normalize(vec3<f32>(r * cos(ph), -(r * sin(ph)), sqrt(max(1.0 - u1, 0.0))));
}

fn light_hit(ro: vec3<f32>, rd: vec3<f32>) -> f32 {
    let oc = ro - frame.light.xyz;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - frame.light.w * frame.light.w;
    let disc = b * b - c;
    if disc < 0.0 {
        return INF;
    }
    let t = -b - sqrt(disc);
    return select(INF, t, t > 0.0);
}

// Direct light at p (normal n) by solid-angle sampling of the light sphere;
// radiance / albedo.
fn direct_light(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let to_c = frame.light.xyz - p;
    let d2 = dot(to_c, to_c);
    let d = sqrt(d2);
    let w = to_c / d;
    let rl = frame.light.w;
    let sin2_max = min(rl * rl / d2, 1.0);
    let cos_max = sqrt(1.0 - sin2_max);
    let u1 = rnd();
    let cos_t = 1.0 - u1 * (1.0 - cos_max);
    let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
    let ph = TAU * rnd();
    let dir = onb_t(w) * (cos(ph) * sin_t) + onb_b(w) * (sin(ph) * sin_t) + w * cos_t;
    let ndl = dot(n, dir);
    if ndl <= 0.0 {
        return 0.0;
    }
    if march_solids(p, dir, 0.0, d - rl).ok {
        return 0.0;
    }
    return frame.shade.x * ndl * 2.0 * (1.0 - cos_max);
}

fn far_ground_radiance(p: vec3<f32>, albedo: f32) -> f32 {
    let to = frame.light.xyz - p;
    let d = max(length(to), frame.light.w * 1.001);
    let cos_max = sqrt(max(1.0 - (frame.light.w / d) * (frame.light.w / d), 0.0));
    let ndl = max(to.z / d, 0.0);
    return albedo * (frame.shade.x * ndl * 2.0 * (1.0 - cos_max) + frame.shade.y * AMBIENT_OCCLUSION);
}

struct Sample {
    l: f32,
    primary: Hit,
}

fn radiance(ro0: vec3<f32>, rd0: vec3<f32>) -> Sample {
    var ro = ro0;
    var rd = rd0;
    var acc = 0.0;
    var throughput = 1.0;
    var primary = MISS;
    let bounces = frame.dims.w;
    for (var depth = 0u; depth <= bounces; depth++) {
        let h = march(ro, rd);
        if depth == 0u {
            let tl = light_hit(ro, rd);
            if tl < INF && (!h.ok || tl < h.t) {
                return Sample(frame.shade.x, MISS);
            }
            primary = h;
        }
        if !h.ok {
            acc += throughput * select(frame.shade.y, frame.shade.z, depth == 0u);
            break;
        }
        let p = ro + rd * h.t;
        var albedo = 0.0;
        if h.id == GROUND {
            albedo = ground_albedo(p, h.t * frame.trace.z / max(-rd.z, GRAZING_MIN));
            if floor_radius(p) > frame.trace.y {
                acc += throughput * far_ground_radiance(p, albedo);
                break;
            }
        } else {
            albedo = solids[h.id].params.y;
        }
        let n = normal(h.id, p);
        let origin = p + n * RAY_OFFSET;
        acc += throughput * albedo * direct_light(origin, n);
        if depth == bounces {
            acc += throughput * albedo * frame.shade.y * AMBIENT_OCCLUSION;
            break;
        }
        throughput *= albedo;
        ro = origin;
        if h.id == GROUND {
            rd = ground_bounce();
        } else {
            rd = cosine_hemisphere(n);
        }
    }
    // the floor's outer rim fades into the background
    if primary.ok && primary.id == GROUND {
        let r = floor_radius(ro0 + rd0 * primary.t) / max(frame.trace.x, 1e-3);
        var k = clamp((r - (1.0 - RIM)) / RIM, 0.0, 1.0);
        if k > 0.0 {
            k = k * k * (3.0 - 2.0 * k);
            acc += (frame.shade.z - acc) * k;
        }
    }
    return Sample(acc, primary);
}

// ------------------------------------------------------------------ one cell

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let mode = frame.pix.x;
    if mode >= 2u {
        // discrete pixels: one invocation per screen pixel
        let sw = frame.pix2.y;
        let sh = frame.pix2.z;
        if gid.x >= sw || gid.y >= sh {
            return;
        }
        let out = gid.y * sw + gid.x;
        // the picture, moved by the burn-in shift
        let ix = i32(gid.x) - frame.pix3.x;
        let iy = i32(gid.y) - frame.pix3.y;
        if mode == 3u {
            pixels[out] = overlaid(gid.x, gid.y, 0xff000000u);
            return;
        }
        var colour = 0xff000000u;
        var id = NO_HIT;
        var depth = INF;
        if mode == 4u {
            // halo-only frame: the picture as last traced
            let t = traced[out];
            colour = 0xff000000u | (t.x & 0x00ffffffu);
            id = t.x >> 24u;
            depth = bitcast<f32>(t.y);
        } else {
            if ix >= 0 && iy >= 0 && ix < i32(frame.dims.x) && iy < i32(frame.dims.y) {
                let c = trace_cell(u32(ix), u32(iy));
                colour = pixel_color(c.lum, c.id, max(frame.misc.w, 1u));
                id = c.id;
                depth = c.depth;
            }
            traced[out] = vec2<u32>((colour & 0x00ffffffu) | (id << 24u), bitcast<u32>(depth));
        }
        pixels[out] = overlaid(gid.x, gid.y, with_halo(f32(ix) + 0.5, f32(iy) + 0.5, colour, id, depth));
        return;
    }
    // the text stream
    let w = frame.dims.x;
    let h = frame.dims.y;
    if gid.x >= w || gid.y >= h {
        return;
    }
    let c = trace_cell(gid.x, gid.y);
    let n = max(frame.misc.w, 1u);
    let level = min(u32(c.lum * f32(n)), n - 1u);
    let lum16 = u32(round(c.lum * 65535.0));
    var depth_bits = 0x7f800000u;
    if c.depth < INF {
        depth_bits = bitcast<u32>(c.depth);
    }
    cells[gid.y * w + gid.x] = vec2<u32>(level | (c.id << 8u) | (lum16 << 16u), depth_bits);
}

struct Traced {
    lum: f32,
    depth: f32,
    id: u32,
}

// Cell (or pixel) (gx, gy): its display luminance (Reinhard + gamma 2.2)
// and the first surface its centre ray hit.
fn trace_cell(gx: u32, gy: u32) -> Traced {
    rng = hash((gy * frame.dims.x + gx) ^ hash(frame.misc.z));
    let spp = max(frame.dims.z, 1u);
    var sum = 0.0;
    var depth = INF;
    var id = NO_HIT;
    for (var s = 0u; s < spp; s++) {
        var jx = 0.5;
        var jy = 0.5;
        if s > 0u {
            jx = rnd();
            jy = rnd();
        }
        let x = f32(gx) + jx;
        let y = f32(gy) + jy;
        let u = (x - frame.proj.y) * frame.proj.x;
        let v = (frame.proj.z - y) / frame.proj.w * frame.proj.x;
        let rd = normalize(frame.cam_fwd.xyz + frame.cam_right.xyz * u + frame.cam_up.xyz * v);
        let smp = radiance(frame.cam_pos.xyz, rd);
        if s == 0u && smp.primary.ok {
            depth = smp.primary.t;
            id = smp.primary.id;
        }
        sum += smp.l;
    }
    let x = max(sum / f32(spp) * frame.shade.w, 0.0);
    var lum = pow(x / (1.0 + x), 1.0 / 2.2);
    if !(lum >= 0.0) {
        lum = 0.0;
    }
    return Traced(min(lum, 1.0), depth, id);
}

// Picture pixel centre (px, py), showing `colour` (solid `id` at `depth`):
// with the halo's nearest visible line drawn over it, anti-aliased.
fn with_halo(px: f32, py: f32, colour: u32, id: u32, depth: f32) -> u32 {
    if frame.halo.w == 0.0 {
        return colour;
    }
    let hw = frame.halo.y;
    let p = vec2<f32>(px, py);
    let groups = u32(frame.halo.z);
    let lines0 = groups * 3u;
    var best = 0.0;
    var best_group = 0u;
    for (var g = 0u; g < groups; g++) {
        let bb = halo_lines[g * 3u];
        if p.x < bb.x - hw - 1.0 || p.y < bb.y - hw - 1.0 || p.x > bb.z + hw + 1.0 || p.y > bb.w + hw + 1.0 {
            continue;
        }
        let info = bitcast<vec4<u32>>(halo_lines[g * 3u + 1u]);
        let tol = halo_lines[g * 3u + 2u].x;
        for (var k = info.x; k < info.x + info.y; k++) {
            let seg = halo_lines[lines0 + k * 2u];
            let d = halo_lines[lines0 + k * 2u + 1u];
            let a = seg.xy;
            let ab = seg.zw - a;
            let len2 = max(dot(ab, ab), 1e-6);
            let t = clamp(dot(p - a, ab) / len2, 0.0, 1.0);
            let dist = length(p - (a + ab * t));
            let cover = clamp(hw + 0.5 - dist, 0.0, 1.0);
            if cover <= best {
                continue;
            }
            // hidden only by something clearly in front of it
            let line_depth = mix(d.x, d.y, t);
            if id == info.w || depth >= line_depth - tol {
                best = cover;
                best_group = g;
            }
        }
    }
    if best <= 0.0 {
        return colour;
    }
    let info = bitcast<vec4<u32>>(halo_lines[best_group * 3u + 1u]);
    let lum = frame.halo.x;
    var hc = 0u;
    if info.z == NONE {
        // the colour under it, at the halo's brightness
        hc = pixel_color(lum, id, max(frame.misc.w, 1u));
    } else {
        hc = pack_rgb(f32((info.z >> 16u) & 0xffu) * lum, f32((info.z >> 8u) & 0xffu) * lum, f32(info.z & 0xffu) * lum);
    }
    return blend(colour, hc, best);
}

fn pack_rgb(r: f32, g: f32, b: f32) -> u32 {
    let ri = u32(clamp(r, 0.0, 255.0) + 0.5);
    let gi = u32(clamp(g, 0.0, 255.0) + 0.5);
    let bi = u32(clamp(b, 0.0, 255.0) + 0.5);
    if (frame.pix.w & 4u) != 0u {
        return 0xff000000u | (bi << 16u) | (gi << 8u) | ri;
    }
    return 0xff000000u | (ri << 16u) | (gi << 8u) | bi;
}

// `under` and `over` (both in the screen's channel order) mixed by `a`.
fn blend(under: u32, over: u32, a: f32) -> u32 {
    let u = vec3<f32>(f32((under >> 16u) & 0xffu), f32((under >> 8u) & 0xffu), f32(under & 0xffu));
    let o = vec3<f32>(f32((over >> 16u) & 0xffu), f32((over >> 8u) & 0xffu), f32(over & 0xffu));
    let m = u + (o - u) * a;
    return 0xff000000u | (u32(m.x + 0.5) << 16u) | (u32(m.y + 0.5) << 8u) | u32(m.z + 0.5);
}

// Screen pixel (sx, sy): what the CPU drew there (the debug panel, the
// stats bar, a whole frame it rendered itself) if it lies in one of the two
// overlay rectangles, else `c`.
fn overlaid(sx: u32, sy: u32, c: u32) -> u32 {
    for (var k = 0u; k < 2u; k++) {
        var r = frame.ov0;
        var off = frame.ov_off.x;
        if k == 1u {
            r = frame.ov1;
            off = frame.ov_off.y;
        }
        let x = i32(sx) - r.x;
        let y = i32(sy) - r.y;
        if r.z > 0 && x >= 0 && y >= 0 && x < r.z && y < r.w {
            let v = overlay[off + u32(y * r.z + x)];
            if (frame.pix.w & 4u) != 0u {
                return 0xff000000u | ((v & 0xffu) << 16u) | (v & 0xff00u) | ((v >> 16u) & 0xffu);
            }
            return 0xff000000u | (v & 0x00ffffffu);
        }
    }
    return c;
}
fn pick(first: u32, count: u32, l: f32) -> u32 {
    if count == 0u {
        return NONE;
    }
    return palette[first + min(u32(l * f32(count)), count - 1u)];
}

// A discrete pixel's colour (PixelPalette::color): its glyph's ink, scaled
// by its luminance.
fn pixel_color(l: f32, id: u32, ramp_len: u32) -> u32 {
    let flags = frame.pix.w;
    let n_bg = frame.pix.y;
    let n_solid = frame.pix.z;
    var c = NONE;
    if (flags & 1u) != 0u {
        if id >= frame.misc.y {
            c = pick(0u, n_bg, l);
        } else if (flags & 2u) != 0u {
            let level = min(u32(l * f32(ramp_len)), ramp_len - 1u);
            c = palette[n_bg + n_solid + id * ramp_len + level];
        } else {
            c = pick(n_bg, n_solid, l);
        }
    }
    if c == NONE {
        c = frame.pix2.x;
    }
    let r = u32(f32((c >> 16u) & 0xffu) * l + 0.5);
    let g = u32(f32((c >> 8u) & 0xffu) * l + 0.5);
    let b = u32(f32(c & 0xffu) * l + 0.5);
    if (flags & 4u) != 0u {
        return 0xff000000u | (b << 16u) | (g << 8u) | r;
    }
    return 0xff000000u | (r << 16u) | (g << 8u) | b;
}
