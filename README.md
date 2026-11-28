# platomon 1.4.0

A terminal system monitor: one path-traced polyhedron per metric, rendered in monochrome ASCII on
concentric rings. Each solid's scale and spin follow its metric; the frame rate (5–150 fps) follows
total activity.

Keys: `s` toggles the stats bar, `l` the legend, `w` the wireframe overlay, `t` the tags, `F1` the debug panel over the left half of the picture (the command line, version and build features, frame rate against its target, render, compose and write times with their peaks, the quality governor's level and load, the renderer in use, CPU or GPU, and the scene and look options); `q` / `Esc` / `Ctrl-C` quit. In the screensaver (and its preview), `F1` shows the same panel over the left half of the screen, in a font of its own (a sixtieth of the screen's height, never smaller than the text, so it stays readable over fine text and at high resolutions), with the CPU budget, text size and draw and present times, instead of ending it; any other key still ends it.

## Launch parameters

```
platomon [OPTIONS]
```

| Option | Syntax | Default |
|---|---|---|
| **General** | | |
| `--config` | `PATH` | `<config dir>/platomon/config.toml` if present |
| `--list` | `solids\|metrics\|aspects\|sensors\|patterns\|gradients` | prints IDs (sensors: this machine's disks/adapters; gradients: colour swatches) and exits |
| `--print-config` | flag | print effective config as TOML |
| `--snapshot` | `COLSxROWS` | render one frame to stdout, no terminal needed |
| `--snapshot-time` | seconds of animation to simulate first | 0.3 |
| `--perf-log` | `PATH`: one CSV line per second (fps, quality, render/write/wait ms, KB per frame, grid) | off |
| `--stats` | flag | stats bar visible at start |
| **Metrics & solids** | | |
| `--show` | `METRIC[,METRIC…]`, max 16 | `cpu,ram,disk,net,gpu` |
| `--solid` | `METRIC=SOLID`, repeatable | built-in mapping |
| `--ring` | `METRIC=N`, repeatable (0 = innermost ring or shell) | automatic |
| `--bind` | `SHAPE\|SIZE\|SPIN\|COLOUR\|COMPLEXITY[\|CURVATURE]`, repeatable, max 12: one solid with its own inputs, replacing `--show` (see *Per-solid inputs*) | off |
| `--delta-period` | seconds a `delta:` input measures the change over, 1–600 | `10` |
| `--curvature` | -10..10: the curvature of every solid that sets none of its own (-10 dished faces, hyperboloid-like; 0 flat; 10 spherical) | `0` |
| `--split-cpu` | one solid per logical core for each solid the whole CPU drives (32 solids at most) | off |
| `--layout` | `rings` (on the ground), `sphere` (concentric spherical shells), `line` (one evenly spaced row), `stack` (one above another in the centre) or `orbit` (one solid in the centre, the others orbiting it) | `rings` |
| `--ordering` | which solid takes which place: `listed` (metric order), `simple` (simplest first), `complex` (most complex first) or `random` | `listed` |
| `--spacing` | neighbour spacing as a multiple of the standard, 0.25–4 (orbits: 10% × this of their radius apart) | `1` |
| `--distance-scale` | each solid's size times 1 + N × (nearest neighbour's distance ÷ standard spacing − 1), −2–2; 0 = off | `0` |
| `--orbit-center` | orbit layout: the solid in the centre, `cpu`, `ram`, `net`, `disk`, `size` (largest now) or `spin` (fastest now) | `cpu` |
| `--orbit-velocity` | orbit layout: angular speed of the innermost orbit, radians/s, 0.05–2 | `0.35` |
| `--orbit-inclination` | orbit layout: tilt of the orbits, degrees, 0–90 | `15` |
| `--orbit-eccentricity` | orbit layout: 0 = circles, up to 0.8 = long ellipses | `0.15` |
| `--orbit-variation` | orbit layout: how much each orbit's spacing, speed, tilt, shape and direction differ, 0–1 | `0.3` |
| **Rendering** | | |
| `--spp` | `N` (1–64) | 2 |
| `--bounces` | `N` (0–8) | 2 |
| `--fps-min` / `--fps-max` | `FPS` | 5 / 150 |
| `--fps-invert` | flag: idle runs at `--fps-max`, full load at `--fps-min` | off |
| `--ramp` | `STR`, darkest first (any length ≥ 2) | `" .,:;-~=+*ox#%&@"` (16 levels) |
| `--char-aspect` | cell width ÷ height | 0.5 |
| `--gpu` | flag: **Space heater**, render on the GPU through Vulkan 1.1 (builds with `--features gpu`); every floor pattern (PCs without Vulkan render on the CPU); `--snapshot` says on stderr which device rendered it and how long it took | off |
| **Display ratio** | | |
| `--aspect` | `fill`, preset or `W:H` | `fill` |
| `--aspect-mode` | `letterbox\|crop` | `letterbox` |
| **Camera** | | |
| `--camera` | `static\|orbit\|focus\|tilt\|resonance` (focus: glide toward the busiest solid; tilt: orbit while rising 20–80° and back; resonance: orbit with three 20° waves per turn) | `static` |
| `--no-floor-clearance` | flag: solids rest just above the floor instead of floating so their largest halo clears it | off |
| `--far-ground` | flag: extended floor, drawn 8x further out toward the horizon; past the normal distance it is shaded in closed form (averaged pattern, exact light, no sampling), about +0–0.4 ms per frame | off |
| `--tag-fixed-length` | flag (beside, below and trailing tags): tag leader lines keep one length, following each solid as it grows and shrinks, and only lengthen to keep a tag off another solid | off |
| `--tag-place` | where tags go: `circle` (round the picture's centre, nearest each solid), `beside`, `corner` (stacked in the corners), `below` or `trailing` (behind moving solids) | `circle` |
| `--auto-distance` | flag: dynamic camera distance, moving in and out so every solid stays in frame (all cameras but focus) | off |
| `--focus-zoom` | `0..1`: 0 = only turn toward the busiest solid, 1 = close-up | 0.6 |
| `--focus-tau` | glide time constant, seconds | 1.5 |
| `--orbit-period` | seconds per revolution | 60 |
| `--orbit-dir` | `random\|cw\|ccw` (seen from above) | `random` |
| `--orbit-radius` | world units | fit to layout |
| `--orbit-height` | world units | fit to layout |
| `--orbit-bob` | vertical drift amplitude, 0 = level | 0.3 |
| `--seed` | `U64`, makes `random` repeatable | random |
| **Background** | | |
| `--bg-lum` | `0..10`: 5 = original, **0 = no background surface** | 5 |
| `--bg-pattern` | `solid\|checker\|diamond\|brick\|triangles\|hex\|radial\|quasicrystal\|rain\|ocean\|julia\|carpet\|clouds\|polka` | `solid` |
| `--bg-contrast` | `0..1`, tile tone spread around `--bg-lum` | 0.5 |
| `--bg-scale` | tile size, world units (a full-size solid is 2 across) | 1.0 |
| `--bg-scroll` | `DX:DY` world units/s across the floor (the x, y plane; z is up, +y away from the camera's starting view), e.g. `0.5:0` or `-0.2:0.3` | `0:0` |
| `--bg-sway` | radius of a circular back-and-forth translation; 0 = off | 0 |
| `--bg-sway-period` | seconds per sway cycle | 10 |
| `--bg-rotate` | `off\|cw\|ccw`, about the camera's focal point (seen from above) | `off` |
| `--bg-rotate-period` | seconds per revolution | 60 |
| `--bg-waves` | quasicrystal: plane waves, 3..24 (its rotational symmetry) | 7 |
| `--bg-wave-speed` | flow of the moving patterns: the quasicrystal's wave phase speed, radians/s (0 = still, negative = reverse); rain, ocean, julia and clouds move in proportion (0.6 = their natural pace) | 0.6 |
| `--bg-drops` | rain: raindrops rippling at once, 1..48 | 12 |
| `--bg-ripple`, `--bg-drop-life` | rain: distance between ripples (tiles); seconds a drop's rings last | 0.9, 4.5 |
| `--bg-wind`, `--bg-chop` | ocean: wind direction (degrees); crest sharpness (0 = plain sine waves) | 30, 0.6 |
| `--bg-julia-shape`, `--bg-julia-iter` | julia: radius of the circle the constant drifts round; iterations (edge detail) | 0.7885, 28 |
| `--bg-carpet-levels` | carpet: sizes of square cut out | 5 |
| `--bg-octaves`, `--bg-roughness` | clouds: noise octaves; each finer octave's strength | 5, 0.5 |
| `--bg-sectors` | radial: sectors round the focal point | 12 |
| `--bg-dot-size`, `--bg-dot-spacing` | polka: each dot's size across and the distance between neighbouring dots, tiles | 0.5, 1 |
| `--bg-dot-variability` | polka: how much dot sizes and places vary, 0..1 (sizes from none to twice `--bg-dot-size`, each dot moved off its lattice point by up to half the spacing) | 0 |
| `--bg-invert-half` | polka: swap light and dark on one side of a line through the centre (the line turns with the floor, but doesn't scroll or sway) | off |
| **Overlays** | | |
| `--legend` | flag: corner panel with a wireframe of each solid and its metric | off |
| `--legend-size` | `1..5` font size: 1–2 terminal text, 3–5 block letters (1–3 cells per font pixel); icons grow with it | 2 |
| `--legend-opacity` | `0..1` panel background: 0 = see-through, 1 = solid | 0.85 |
| `--legend-pos` | `tl\|tr\|bl\|br` (or `top-left`, …) | `tl` |
| `--wireframe` | flag: each solid's visible edges drawn in inverted luminosity | off |
| **Colour** | | |
| `--color` | `off`, a preset (`--list gradients`) or 2+ hex stops, e.g. `"#000:#f80:#fff"` | `off` |
| `--color-by` | `lum` (brightness → gradient), `load` (each solid by its metric) or `spin` (each solid by its spin speed and direction) | `lum` |
| `--colors` | palette size: `16`, `32`, `64` or `128` colours per gradient | `16` |
| `--bg-color` | background colouring, independent of the solids: `auto`, `off`, `match` (the solids' gradient by brightness) or a gradient of its own | `auto` |
| `--color-reverse` | flag: run the gradient backwards | off |

Any `--legend-*` option turns the legend on. `--color-by` or `--color-reverse` without `--color`
picks a default gradient: `inferno` for `lum`, `traffic` for `load`.

The orbit circles the centroid of all solids and advances with wall-clock time, so its speed doesn't
depend on fps.

### Focus camera

`--camera focus` starts from the static overview and glides toward whichever solid's metric is
busiest, looking straight at it. It moves along the line from that solid back to the overview
position: `--focus-zoom 0` stays at overview distance and only turns, `1` comes in to a close-up
(5 units from the solid's centre). Movement is exponentially smoothed with `--focus-tau` seconds.
To stop the camera darting between near-equal metrics, a new solid must lead the current one by 5
percentage points for a full second before the camera switches.

### Tilt camera

`--camera tilt` orbits like `orbit` (one turn per `--orbit-period`, direction from `--orbit-dir`)
and at the same time lifts along a vertical arc over the orbit point, from 20° to 80° elevation
above the look-at point (never straight down at the floor), at the overview distance throughout.
The rise takes one orbital period, easing in and out; at each end it holds for half a period while
the orbit carries on, then travels back. It starts on its way up at the overview's elevation.

### Floating solids and the extended floor

Solids float above the floor at a fixed height (rings) or with the whole sphere lifted, so that
even the largest sound halo (1.8x a solid's radius, the ±60% swing) stays clear of it; the camera
aims correspondingly higher. This is the *minimum floor distance* (on by default; `--no-floor-clearance`,
`floor_clearance=off` or the setting in the Appearance group turn it off): off, the solids rest just
above the floor at any size (they never touch it, but a large halo can dip into it). `--far-ground` draws the floor 8x further out, filling the band of sky
that wide layouts and low camera angles otherwise show at the horizon. Out there the pattern has
already faded to its mean tone and no solid's shadow reaches, so instead of sampling the light,
tracing shadow rays and bouncing, each far floor point gets the light's direct contribution in
closed form plus the averaged sky term: about 0–0.4 ms per frame at 640×240 cells.

**Seen from far above.** The floor's reach (`max_dist` in the config file, *Render distance* in the screensaver's Rendering
dialog; 50 by default, 8x with the extended floor) is a radius round the scene's centre measured across the floor, not a distance along each
ray, so a camera high above the floor (a large orbit height, the dynamic camera distance backing
out) sees the same floor as one near it; before, a camera more than 50 away lost the floor and then
the solids too. Solids are found however far away the camera is. The floor's outer 15% fades into
the background, so its edge is soft rather than a ragged circle. Patterns fade to their mean tone
by how much floor each character cell covers: the cell's angle (from the field of view and the
rendering resolution, so finer text keeps finer tiles) times the distance, stretched at grazing
angles, which removes the moiré and flicker where tiles shrink below a cell toward the horizon or
seen from high up.
### Dynamic camera distance

`--auto-distance` (*Dynamic camera distance* in the screensaver) keeps every solid in frame by
moving the camera in and out along its line to the look-at point. Each frame it finds the closest
distance (0.35x to 3x the camera's own) at which every solid's bounding sphere, at its current size,
fits inside the view with 10% to spare, and backs out at once when a solid would leave the frame but
comes back in gently (1.5 s). It works with the static, orbit, tilt and resonance cameras; the focus
camera keeps its own close-ups.

### Resonance camera

`--camera resonance` orbits on a third-order (3:1) disc resonance: while it circles once, its
elevation rises and falls sinusoidally three times over a 20° range centred on the overview's
elevation, and its distance breathes ±8% in step, so the closed path is a three-lobed rosette that
repeats every orbit.

### Line and stack layouts, ordering and spacing

`--layout line` puts the solids in one evenly spaced row along the ground, floating at the same
height as the rings; `--layout stack` sets them one above another in the centre, the lowest lifted
so its largest halo clears the floor, with the camera backed off to frame the column. `--ordering`
decides which solid takes which place: `listed` keeps the metric order, `simple` puts the simplest
solid (fewest edges, faces breaking ties) first and the most complex last, `complex` the reverse,
and `random` shuffles them (the same for a given seed). The first place is the inner ring's front,
the inner shell, the line's left end or the stack's bottom. `--spacing` scales the distance between
neighbours (and between rings and shells).

`--distance-scale N` (−2 to 2, default 0 = off) sizes each solid by the room around it: its size is
multiplied by 1 + N × (d ÷ the standard spacing − 1), where d is the distance to its nearest
neighbour, clamped to 0.2×–2.5×. A solid exactly the standard spacing from its neighbour keeps its
size; with a positive N, solids with more room grow and crowded ones shrink, and a negative N does
the reverse. It works in every layout (wider `--spacing` gives it more to work with); in the orbit
layout the distances change as the solids move, so their sizes change smoothly with them. Floating
heights and camera framing allow for the largest size it can give.

### Orbit layout

`--layout orbit` puts one solid, the reference, in the centre and the others on orbits around it,
each orbit 10% of its radius larger than the one inside it (`--spacing` stretches that: 2 = 20%,
0.5 = 5%). `--orbit-center` picks
the reference: `cpu`, `ram`, `net` or `disk` (that metric's solid; a network adapter or disk device
when the main one isn't shown) holds the centre for the whole run; `size` gives it to whichever
solid is largest at the moment and `spin` to whichever spins fastest. A solid that has led by a
clear margin for 1.5 s takes over: it glides in to the centre and the old reference glides back out
onto its orbit on a critically damped spring (about two seconds, no overshoot), so nothing jumps.

The orbits are Kepler ellipses, and solids move along them by Kepler's equation: they speed up as
they swing in close to the centre and slow down far out, smoothly. `--orbit-velocity` is the
innermost orbit's angular speed in radians per second (default 0.35, about 18 s per turn); outer
orbits are slower, as the square root of their size. `--orbit-inclination` tilts the orbits from
the floor (degrees, 0 = flat, 90 = upright), and `--orbit-eccentricity` stretches them from circles
(0) to long ellipses (0.8); every orbit's closest point stays a full solid spacing clear of the
centre. `--orbit-variation` (0..1) spreads the other settings: at 0 every orbit lies in one plane
with one shape, and toward 1 each orbit gets its own size step, speed (±60%), tilt (±30°),
eccentricity (±0.3), orientation and starting point (the same for a given seed). The centre is
raised so the lowest point of any orbit keeps the largest halo clear of the floor, and the camera
backs off further the steeper the orbits tilt. Orbits may cross, so two solids can briefly pass
through each other.

### Sphere layout

`--layout sphere` places the solids on concentric spherical shells instead of rings on the ground.
`--ring` (and automatic placement, 8 per shell) picks the shell; each shell spreads its solids
evenly (a Fibonacci lattice) and is sized so neighbours keep the same spacing as on the rings. The
whole cluster floats just above the ground plane, so the background, its patterns and contact
shadows still work; the camera frames it from further back. Solids grow and shrink about fixed
centres rather than resting on the ground.

### Colour

Colour is off by default (pure monochrome). `--color GRADIENT` samples a gradient into a fixed
palette of `--colors` 16 (default), 32, 64 or 128 colours, and every coloured cell takes one of
them. The solids' colour comes from one of these sources:

- `--color-by lum`: the cell's brightness, so dark cells take the first stop and bright cells the
  last. With 16 colours and the 16-glyph ramp that is one colour per glyph; larger palettes add
  steps between glyph levels (smoother, but more cells change colour from frame to frame).
- `--color-by load`: each solid takes the colour at its metric's load (idle = first stop, full =
  last stop), shaded by brightness. Legend icons take their solid's colour.
- `--color-by spin`: each solid by its spin as seen from the camera. The middle of the gradient is
  still (or spinning edge-on), toward the last stop is faster counter-clockwise, toward the first
  stop faster clockwise; shaded by brightness.
- `sound` (screensaver only): each solid by the loudness of its own frequency band of the default
  output device, low frequencies on the simplest solids and high frequencies on the most complex
  (the same placement as *Halo bounce by frequency*); shaded by brightness.

For the shaded sources the palette is positions along the gradient × brightness shades: 16 colours
are 8 positions × 2 shades, larger palettes have 4 shades (8, 16 or 32 positions).

`--bg-color` maps the background (floor and sky) on its own, always by brightness: `auto` (default)
colours it with the solids' gradient when they are coloured by brightness and leaves it uncoloured
otherwise, `off` leaves it uncoloured, `match` uses the solids' gradient whatever colours the
solids, and a preset or hex stops give it a gradient of its own (in its own palette of the same
size). With `--color off` and a background gradient only the background is coloured. Legend text
keeps the terminal's default colour.

| Gradient | Colours |
|---|---|
| `inferno` | black, purple, red, orange, pale yellow |
| `viridis` | purple, blue, teal, green, yellow |
| `plasma` | indigo, magenta, orange, yellow |
| `fire` | black, deep red, orange, gold, white |
| `ice` | navy, blue, cyan, white |
| `ocean` | deep blue to aqua |
| `sunset` | violet, magenta, coral, gold |
| `matrix` | phosphor green |
| `amber` | amber monochrome monitor |
| `grey` | neutral grey ramp |
| `traffic` | green, yellow, red (made for `--color-by load`) |
| `rainbow` | red through violet |

Custom gradients take two or more hex stops (`#rgb` or `#rrggbb`, `#` optional), evenly spaced,
separated by `:`, `,`, `;`, `/` or spaces. Colour escapes add to the bytes written per frame,
so very large grids on slow consoles may lose some fps. (24-bit colour and `--color-depth` were
removed: a `color_depth` key in an older config file is ignored.)

### Luminosity levels

Each pixel's radiance is tone-mapped (Reinhard, then gamma 2.2) to a luminance from 0 to 1. The
ramp splits that range into equal buckets, one per glyph. The default ramp has 16:

| # | Glyph | Luminance | # | Glyph | Luminance |
|---:|:---:|---|---:|:---:|---|
| 0 | ` ` | 0.0000 – 0.0625 | 8 | `+` | 0.5000 – 0.5625 |
| 1 | `.` | 0.0625 – 0.1250 | 9 | `*` | 0.5625 – 0.6250 |
| 2 | `,` | 0.1250 – 0.1875 | 10 | `o` | 0.6250 – 0.6875 |
| 3 | `:` | 0.1875 – 0.2500 | 11 | `x` | 0.6875 – 0.7500 |
| 4 | `;` | 0.2500 – 0.3125 | 12 | `#` | 0.7500 – 0.8125 |
| 5 | `-` | 0.3125 – 0.3750 | 13 | `%` | 0.8125 – 0.8750 |
| 6 | `~` | 0.3750 – 0.4375 | 14 | `&` | 0.8750 – 0.9375 |
| 7 | `=` | 0.4375 – 0.5000 | 15 | `@` | 0.9375 – 1.0000 |

The original 10-level ramp is `--ramp " .:-=+*#%@"`.

### Background

The ground plane's mean brightness follows `--bg-lum` linearly: 5 is the original ground and 10 is
twice as bright. At 0 the plane is removed entirely, so the solids float in a black void with no
ground or contact shadows. Patterns alternate their tiles above and below that mean by
`--bg-contrast`, so changing the pattern doesn't change the overall brightness. `hex` uses three
tones, and `radial` is a polar checkerboard centred on the focal point. Far-away tiles smaller than
a character cell fade to the mean instead of flickering.

Motion is set in the pattern's own frame and runs on wall-clock time. `--bg-scroll` translates at a
constant velocity, `--bg-sway` adds a circular back-and-forth, and `--bg-rotate` spins the pattern
about the camera's focal point (the layout centroid the camera looks at). Rotation and the
`--camera orbit` direction use the same `cw`/`ccw` convention, so they can turn with or against
each other. With `--bg-pattern solid` (the default) motion has nothing visible to move.

| Pattern | Description |
|---|---|
| `solid` | uniform (original look) |
| `checker` | square checkerboard |
| `diamond` | checkerboard rotated 45° |
| `brick` | checkerboard, alternate rows offset half a tile |
| `triangles` | squares split on the diagonal, alternating |
| `hex` | hexagonal tiles, three tones |
| `radial` | rings × 12 sectors around the focal point |
| `quasicrystal` | `--bg-waves` plane waves at evenly spaced angles, summed and folded into bands: a never-repeating, many-fold symmetric tiling that flows (`--bg-wave-speed`) |
| `rain` | a raindrop simulation: `--bg-drops` drops land at random places, each sending out a ring of ripples (a short wave train behind a front spreading at about 1.4 tiles/s, ripples 0.9 tiles apart) that weakens as it spreads and fades over its 3–6 s life, when a new drop lands somewhere else; overlapping rings add up |
| `ocean` | an approximate ocean surface: 12 waves round a random wind direction, wavelengths from 4 tiles down by a factor 0.76, taller the longer, each moving at its deep-water speed (√(g/k): long swells overtake short chop), with sharpened crests and flat troughs |
| `julia` | the Julia set fractal (4 tiles per unit), in bands by how fast points escape, its constant drifting slowly round a circle of radius 0.7885 so the set keeps changing shape |
| `carpet` | the Sierpinski carpet fractal: squares cut out at five scales, the smallest a third of a tile |
| `clouds` | fractal (Brownian) noise: five octaves of value noise, each twice as fine and half as strong, drifting |
| `polka` | polka dots `--bg-dot-size` across on a triangular lattice `--bg-dot-spacing` apart (rows offset half a spacing, each dot with six equidistant neighbours); `--bg-dot-variability` spreads both (random sizes, dots nudged off the lattice); `--bg-invert-half` inverts one half of the floor, dark dots on light. Dots much smaller than a tile fade to the floor's mean tone sooner with distance, so they don't flicker |

### Legend and wireframe

The legend lists every slot in `--show` order: a small wireframe of its solid, turning with the
solid and seen from the camera's direction (visible edges as `- | / \`, hidden edges dotted),
next to the live reading (`CPU 37%`, `NIC Wi-Fi ↓1.0 ↑0.5 MB/s`). Sizes 3–5 draw the label in
3×5 block letters with a compact reading (`NIC WI-FI 1.5M`, read + write MB/s). A terminal can't
change its font for part of the screen, so the size is measured in cells. When the entries don't
fit in one column they wrap into more columns, and entries that still don't fit are left out.

`--wireframe` traces each solid's edges into the picture and inverts the luminosity under them:
dark on bright faces, bright on shadow or a black background. Mid-grey cells are pushed at least
6 glyph levels away, so edges stay visible everywhere. Edges on the far side of a solid, or behind
another solid, are hidden.

### Metrics

| Metric | Scale | Spin | Default solid |
|---|---|---|---|
| `cpu[:N]` | utilisation (minus platomon's own); `cpu:N` one logical core | utilisation | icosahedron |
| `ram` | used ÷ total | rate of change | cube |
| `disk` | all disks, read+write | Y = read, X = write | octahedron |
| `net` | all adapters, rx+tx | Y = rx, X = tx | tetrahedron |
| `gpu` | busiest GPU's utilisation | memory-controller util, else utilisation | dodecahedron |
| `nic[:NAME]` | one adapter, rx+tx | Y = rx, X = tx | rhombic-dodecahedron |
| `disk-dev[:NAME]` | one disk/volume, read+write | Y = read, X = write | truncated-cube |
| `cpu-temp`, `gpu-temp` | temperature, 30 °C → 0 to 95 °C → 1 | temperature | truncated-tetrahedron, truncated-dodecahedron |
| `cpu-freq`, `gpu-freq` | clock ÷ the most it reaches (the CPU's nominal clock, the GPU's rated maximum, else 1 GHz, raised by any higher clock seen) | clock | cuboctahedron, icosidodecahedron |
| `cpu-volt`, `gpu-volt` | core voltage across the range seen since the start (at least 0.25 V wide) | voltage | truncated-octahedron, rhombic-triacontahedron |

`nic` and `disk-dev` without a name follow the busiest instance and can appear several times with
different names. `platomon --list sensors` shows the names this machine has. Disks also match by
mount point, e.g. `disk-dev:C:\` or `disk-dev:/home`.

The first seven work on Linux, macOS and Windows with any GPU vendor. The temperatures, clocks and
voltages depend on what the machine reports:

| | Windows | Linux |
|---|---|---|
| CPU temperature | LibreHardwareMonitor / OpenHardwareMonitor (package temperature) while it runs, else the hottest ACPI thermal zone (many desktops have none) | thermal zones, `k10temp` / `coretemp` |
| CPU clock | PDH: nominal clock × *% Processor Performance* (so boost shows) | the cores' mean clock |
| CPU voltage | LibreHardwareMonitor / OpenHardwareMonitor only (the CPU's core voltage, else the motherboard's Vcore) | hwmon `Vcore` |
| GPU temperature, clock | NVIDIA: NVML. AMD: the driver's ADL (Overdrive 8 on Navi and later, 5/6 on older GCN such as the HD 7000s). Else the hardware monitor | `amdgpu` hwmon |
| GPU voltage | AMD: ADL. Else the hardware monitor (NVIDIA reports none) | `amdgpu` hwmon |

`platomon --list sensors` prints what this machine gives. When a metric is unavailable on a
particular machine, its solid sits at minimum scale with idle spin and the stats bar shows `n/a`.

**Split CPU** (`--split-cpu`, `split_cpu=on`, the Metrics dialog's *Split CPU into cores*): every
solid driven by the whole CPU becomes one solid per logical core (`cpu:0`, `cpu:1`, ...), alike in
shape and in its other inputs; their tags read `CPU0 12%`, `CPU1 40%`. Platomon's own share comes off
each core alike. At most 32 solids in all: with more cores than fit, the first cores that do.

### Per-solid inputs

Normally each solid's metric drives its size, spin and colour. With `--bind` (or `bindings = [...]`
in the config file, or the screensaver's *Metrics* dialog) each solid gets its own inputs instead,
written `SHAPE|SIZE|SPIN|COLOUR|COMPLEXITY[|CURVATURE]`:

- `SHAPE`: a solid id, or `auto` (the metric's own solid while it is free, else the first free one).
- Each input: a metric (`cpu`, `nic:Ethernet`, ...), `delta:METRIC`, or `off`. Missing spin and
  colour inputs follow size, and a missing complexity input is off.
- **Size** sets the solid's size (off: always the smallest). **Spin** sets its spin (off: idle
  spin). **Colour** sets its colour when colouring by load (off: the gradient's start).
- **Complexity** changes the solid's shape: at rest it is `SHAPE` (the tetrahedron for `auto`), and
  more load steps it up through solids with more edges, to the most complex at full load. It only
  steps once the input is well past the middle between two shapes, so it does not flicker.
- **Curvature** (the optional sixth field) bends the solid's faces, from -10 to +10. 0 is the
  flat-faced solid. Toward +10 the faces bulge out until, at +10, it is the sphere through its
  corners. Toward -10 each face is dished in by a sphere through its corners, so the faces cave in
  and the edges pinch into concave arcs (hyperboloid-like); at -10 a dish is 60% of the way to the
  centre (or 80% of the face's corner radius, whichever is less). `VALUE` alone is fixed;
  `VALUE@INPUT` (`-8@cpu-temp`, `10@delta:gpu`) is flat while the input is idle and reaches
  `VALUE` at full load, following it every frame. `--curvature` sets it for every solid that sets
  none of its own. Both stay true distance bounds, so curved solids march, shadow and render on
  the GPU like flat ones; a dished solid costs about twice a flat one to trace.
- **`delta:`** uses how much the metric changed over the delta period (`--delta-period`, 10 s by
  default) instead of its value: the difference from the value that long ago, scaled to the largest
  change lately (which fades by 2% a second, never below 5% of the range). A steady load reads as 0,
  and a rise or fall as up to 1.

Several solids can share a metric, and a metric is read once however many solids use it. The tag,
the orbit centre and the activity weight follow a solid's size input (else its first input).

```powershell
platomon --bind "auto|cpu" --bind "cube|ram|delta:ram|ram" --bind "auto|delta:net|net|delta:net|cpu" --delta-period 30
platomon --bind "auto|cpu|cpu|cpu|off|10@cpu" --bind "cube|gpu|gpu|gpu|off|-8@gpu-temp" --curvature 4
```

### Solids

| Family | IDs |
|---|---|
| Platonic | `tetrahedron`, `cube`, `octahedron`, `dodecahedron`, `icosahedron` |
| Archimedean | `truncated-tetrahedron`, `cuboctahedron`, `truncated-cube`, `truncated-octahedron`, `rhombicuboctahedron`, `icosidodecahedron`, `truncated-icosahedron`, `truncated-dodecahedron` |
| Catalan | `rhombic-dodecahedron`, `rhombic-triacontahedron` |

### Rings

Solids are placed on concentric rings. Without `--ring`, slots fill ring 0 (up to 8), then ring 1,
and so on. Ring numbers only set the order of the rings; empty ones are skipped, so `--ring nic=5`
on its own just makes an outer ring. A ring holding a single solid puts it at the centre. Odd rings
are offset by half a step so neighbouring rings interleave.

### Aspect presets

`vga` 4:3, `svga` 4:3, `xga` 4:3, `sxga` 5:4, `wxga` 16:10, `hd` 16:9, `wxga+` 16:10, `fhd` 16:9,
`wuxga` 16:10, `qhd` 16:9, `uwfhd` 64:27, `uwqhd` 43:18, `uhd` 16:9, `square` 1:1, or any `W:H`.
`letterbox` shrinks the picture to the ratio inside the terminal; `crop` fills the terminal with a
picture framed for the ratio and cuts off the overflow.

### Examples

Instance names (`nic:NAME`, `disk-dev:NAME`) are machine-specific; `platomon --list sensors` prints
yours. The examples use Windows names (`Ethernet`, `C:\`); on Linux use e.g. `nic:eth0` and
`disk-dev:/` or `disk-dev:nvme0n1p2`. Quote any argument that contains `\` or spaces, and quote
`--show` lists in PowerShell, which otherwise splits `cpu,ram,disk` into separate arguments.

**Getting started**

```
platomon --stats                                   # default 5 solids, stats bar on
platomon --list sensors                            # this machine's disks and adapters
platomon --list solids                             # all 15 solids with face/vertex counts
platomon --print-config > my.toml                  # start a config from the current settings
```

**Choosing metrics, solids and rings**

```
# CPU alone in the centre, everything else on a ring around it
platomon --show "cpu,ram,disk,net,gpu" --ring cpu=0 --ring ram=1 --ring disk=1 --ring net=1 --ring gpu=1

# One solid per disk on an outer ring, all truncated octahedra except C:
platomon --show "cpu,ram,gpu,disk-dev:C:\,disk-dev:D:\,disk-dev:E:\" --ring disk-dev=1 --solid disk-dev=truncated-octahedron --solid "disk-dev:C:\=rhombicuboctahedron"

# Two adapters, compared side by side
platomon --show "cpu,nic:Ethernet,nic:Wi-Fi" --solid nic:Ethernet=rhombic-dodecahedron --solid nic:Wi-Fi=rhombic-triacontahedron --stats

# Archimedean-only theme for the classic five metrics
platomon --solid cpu=truncated-icosahedron --solid ram=cuboctahedron --solid disk=truncated-octahedron --solid net=truncated-tetrahedron --solid gpu=icosidodecahedron

# All 15 solids on three rings: 1 in the centre, 6 in the middle, 8 outside.
# Named instances that don't exist on a machine simply show n/a at minimum size.
platomon --camera orbit --orbit-period 45 --stats --show "cpu,ram,disk,net,gpu,nic:Ethernet,disk-dev:C:\,nic,disk-dev,disk-dev:D:\,disk-dev:X:\,disk-dev:Z:\,nic:Wi-Fi,nic:vEthernet,nic:Bluetooth" --ring cpu=0 --ring ram=1 --ring disk=1 --ring net=1 --ring gpu=1 --ring nic:Ethernet=1 --ring "disk-dev:C:\=1" --ring nic=2 --ring disk-dev=2 --solid cpu=icosahedron --solid ram=cube --solid disk=octahedron --solid net=tetrahedron --solid gpu=dodecahedron --solid nic:Ethernet=rhombic-dodecahedron --solid "disk-dev:C:\=truncated-cube" --solid nic=rhombic-triacontahedron --solid disk-dev=cuboctahedron --solid "disk-dev:D:\=truncated-octahedron" --solid "disk-dev:X:\=rhombicuboctahedron" --solid "disk-dev:Z:\=icosidodecahedron" --solid nic:Wi-Fi=truncated-icosahedron --solid nic:vEthernet=truncated-dodecahedron --solid nic:Bluetooth=truncated-tetrahedron
```

**Camera and display ratio**

```
platomon --camera orbit --orbit-period 90 --aspect uwqhd               # slow orbit, ultrawide letterbox
platomon --camera orbit --orbit-dir ccw --orbit-bob 0 --aspect fhd     # level ccw circle, 16:9
platomon --camera orbit --seed 7                                       # random direction, same every run
platomon --aspect 21:9 --aspect-mode crop                              # fill the terminal, framed for 21:9
platomon --aspect vga --orbit-height 9 --orbit-radius 7                # 4:3, steep top-down static view
platomon --camera orbit --orbit-period 20 --orbit-radius 14 --orbit-height 3 --orbit-bob 1.5   # low, wide, bobbing fly-around
```

**Background**

```
platomon --bg-lum 1                                                    # dark floor, solids pop
platomon --bg-lum 0 --camera orbit                                     # no floor: solids floating in a void
platomon --bg-lum 8 --bg-pattern checker --bg-contrast 0.8 --bg-scale 2
platomon --bg-pattern hex --bg-scale 1.2 --bg-scroll 0.3:0.3           # three-tone hexes drifting diagonally
platomon --bg-pattern brick --bg-sway 0.6 --bg-sway-period 6           # floor swaying in a slow circle
platomon --bg-pattern quasicrystal --bg-lum 7 --bg-contrast 1 --bg-scale 3   # flowing 7-fold quasicrystal floor
platomon --bg-pattern quasicrystal --bg-waves 5 --bg-wave-speed 0 --bg-rotate ccw  # still Penrose-like 5-fold floor, turning
platomon --bg-pattern rain --bg-lum 7 --bg-contrast 1 --bg-drops 20        # a shower of ripples
platomon --bg-pattern ocean --bg-lum 6 --bg-contrast 0.9 --bg-scale 2      # rolling swell
platomon --bg-pattern julia --bg-lum 7 --bg-contrast 1 --bg-wave-speed 2   # a Julia set changing shape quickly
platomon --bg-pattern triangles --bg-scale 0.6 --bg-contrast 1 --bg-scroll -0.5:0
platomon --bg-pattern radial --bg-rotate ccw --bg-rotate-period 20 --camera orbit --orbit-dir cw   # floor and camera turn against each other
platomon --bg-pattern diamond --bg-rotate cw --bg-rotate-period 30 --bg-scroll 0.4:0 --bg-sway 0.5 # rotate, scroll and sway at once
```

**Focus camera, sphere layout and colour**

```
platomon --camera focus                                                # glide toward the busiest solid
platomon --camera focus --focus-zoom 1 --focus-tau 3                   # slow, full close-ups
platomon --camera tilt --orbit-period 40                               # orbit, rising 20-80 degrees over 40 s
platomon --camera resonance --orbit-period 60                          # three 20-degree waves per minute-long orbit
platomon --layout sphere --show "cpu,ram,disk,net,gpu,nic,disk-dev"    # one shell of 7
platomon --layout sphere --ring cpu=0 --ring ram=1 --ring disk=1 --ring net=1 --ring gpu=1   # CPU at the centre of a shell
platomon --color inferno                                               # brightness through inferno
platomon --color-by load                                               # each solid green → red with its load (traffic)
platomon --color viridis --color-by load --colors 64                   # 64-colour palette
platomon --color plasma --color-by spin --bg-color ice                 # solids by spin, floor in ice
platomon --color-by load --bg-color off                                # solids only, background uncoloured
platomon --color "#001020:#0080ff:#ffffff" --bg-lum 1                  # custom three-stop gradient
platomon --layout sphere --camera focus --color-by load --legend --wireframe   # everything at once
```

**Legend and wireframe**

```
platomon --legend --wireframe                                          # both overlays, default size/corner
platomon --legend-pos br --legend-size 1 --legend-opacity 0.5          # compact, half see-through, bottom right
platomon --legend-pos tr --legend-size 4 --legend-opacity 1            # block-letter labels on a solid panel
platomon --wireframe --bg-lum 0 --camera orbit                         # bright edges on solids floating in a void
platomon --show "cpu,ram,disk,net,gpu,nic,disk-dev" --legend-size 3 --legend-pos bl --wireframe --stats
```

**Shading and performance**

```
platomon --ramp " .:-=+*#%@"                                           # the original 10-level ramp
platomon --ramp " ░▒▓█"                                                # Unicode block shading (needs a font with these glyphs)
platomon --spp 4 --bounces 3 --fps-max 60                              # smoother, slower, capped at 60 fps
platomon --spp 1 --bounces 1 --fps-max 30                              # light on CPU
platomon --fps-min 30 --fps-max 30                                     # constant 30 fps, ignore activity
platomon --fps-invert --fps-max 60                                     # fast when idle, slows to 5 fps under load
```

**Snapshots (no terminal needed)**

```
platomon --snapshot 120x40 > frame.txt                                 # one frame of the default scene
platomon --snapshot 160x48 --snapshot-time 10 --camera orbit --orbit-period 40 --bg-pattern radial --bg-rotate ccw   # 10 s into an animation
```

**Maximum density fullscreen (Windows console)**

On a 2560×1440 display the classic Windows console's smallest bitmap font (Terminal, 4×6 px)
gives a 640 × 240 grid. Cells that size are 4:6 rather than 1:2, so pass the matching
`--char-aspect`. Set the font in the window's Properties › Font, press Alt+Enter for fullscreen, then:

```
platomon --char-aspect 0.6667 --bg-lum 1 --camera orbit --stats
```

At that resolution expect the adaptive quality to drop to `q-1`/`q-2` and tens of fps rather than 150.

## Windows screensaver

**Download:** `platomon.scr` and `platomon-config.exe` are attached to the
[v1.4 release](https://github.com/truncated-hypercube-emporium/platomon-scr/releases/tag/v1.4)
(64-bit Windows 10/11, no installer, no dependencies). To build them yourself,
`scripts\build-scr.ps1` writes both to `dist\`.

To install, copy both files to `C:\Windows\System32` (administrator rights needed), or right-click
`platomon.scr` › Install, then choose **platomon** in Settings › Personalization › Lock screen ›
Screen saver. `platomon-config.exe` opens the same settings dialog as the Settings… button there.
The `install` folder has both files and `install-platomon-scr.bat`, which moves them there (asking for
administrator rights) and then adds Windows Defender exclusions for both installed files (remove them
later in Windows Security › Virus & threat protection › Exclusions).

**New in 1.4:**

- **Tilt camera**: orbits while rising from 20° to 80° elevation in one orbital period, holding half a period at each end, then back (*Camera* › Tilt, `--camera tilt`).
- **Resonance camera**: orbits on a third-order disc resonance, three sinusoidal 20° rises and falls per turn with the distance breathing in step (*Camera* › Resonance, `--camera resonance`).
- **Palettes instead of 24-bit colour**: 16 (default), 32, 64 or 128 colours per gradient (*Palette size*, `--colors`).
- **New colour sources**: each solid by its **spin** (speed and direction seen from the camera) or by the **sound** of its own frequency band, low frequencies on the simplest solids and high on the most complex.
- **Background colour on its own**: auto, off, the solids' gradient, or a gradient of its own (*Background colour*, `--bg-color`).
- **Settings dialog**: check boxes and sliders, one-word option names, *Tuning*, *Layout* (ordering, spacing) and *Rendering* dialogs, a live **Preview** (windowed, or *Fullscreen*), a **configuration log** of the last 10 OKs (each titled with a random word; `+` keeps one for good in `configuration.log`).
- **Halo size swing** is ±10% (was ±5%), ±30% or the new ±60%.
- **Faster colour**: a palette-sized tile cache, a single glyph-and-colour pass and a tone-curve table make the colour pipeline 35–45% faster.
- **Quasicrystal background** (`quasicrystal` pattern, with wave count and flow speed settings), and tags no longer cover the legend.

**New in 1.3.23:**

- **Block-letter tags on small text**: below 10 px cells, tags switch to three-row block letters made of ordinary glyphs, larger on finer text (*Tag size*: automatic, always one line, or block letters 1x to 4x; INI `tag_text`), in a random font each run with *Random font*.
- **Tag positions**: tags on a circle round the picture (each gliding to the point nearest its solid), beside their solids, stacked in the corners, below their solids or trailing behind them (INI `tag_place`).
- **Tag boxes keep off the solids**: each tag box slides along its leader line as close to its solid as it can while staying clear of it (full size, halo room) and of every other solid.
- **Luminosity levels** are 8, 16 (default) or 32: the random choice is gone and the 10-level ramp became an 8-level one (`" .:-=+#@"`). Saved `auto` reads as 16 and `10` as 8.

**New in 1.3.21:**

- **Tags follow their solids**: instead of fixed places on a ring, each tag travels with its solid, placed for the solid at full size so it doesn't move as the solid grows and shrinks with its load (only the camera moves it), three rows past the halo's room. Leader lines are short diagonals that never cross any tag's text.

**New in 1.3.2:**

- **Solid tags** (on by default; *Solid tags* in the dialog): each solid's metric (`__RAM 42%__`)
  travels with its solid on the diagonal pointing from the inside out, toward the picture's corner
  nearest the solid, clear of the room its halo needs (a tag blocked there takes the other corner on
  the same side, never one toward the centre). The text runs outward and a short diagonal leader line (`/`, `\`, or `|`
  when steep; never horizontal) spans only the gap from the tag's inner corner to the solid's halo,
  wireframe or surface. Tags keep their side until a solid is well past the centre, avoid each
  other and the picture's edges, and are drawn over everything, halo-only frames included.
- **Tag colour** (*Tag colour*): the complementary hue of the colour theme (default), a tint of the
  theme itself, or white; every choice is lifted to at least 11:1 contrast against the black
  background. Monochrome tags use the normal text colour.
- **About twice as fast with a background plane**: rays skip solids they cannot reach (bounding
  spheres), and two per-frame lookup tables list the solids each screen tile can show and the
  solids that can shadow each part of the ground; ground bounce and light-sampling directions come
  from precomputed tables. A 640×240 frame with 16 solids and a checkerboard renders in about
  6.1 ms instead of 14 ms (`cargo run --release --example ground_bench`). With the same CPU budget
  the screensaver uses the gain for smaller text (more cells) and a higher frame rate.

**New in 1.3.1:**

- **Halo bounce by frequency** (on by default): each solid's halo follows its own part of the
  spectrum, the simplest solid in the scene the bass and the most complex the treble, on a gradient
  in between. *Halo bounce source* › Overall level brings back one size for all.

**New in 1.3:**

- **Sound halo**: a second wireframe of each solid, 1.2x its size, that grows and shrinks with the
  default output device's level (±10%, ±30% or ±60% of the radius with *Halo size swing*) and gets brighter
  with low frequencies; it updates at the display's refresh rate between full frames.
- **Faster drawing**: glyph tiles copied straight into the back buffer instead of GDI text calls —
  a full 2560×1440 frame now takes about 1.5 ms to draw instead of 40–80 ms.
- **Text density** (default double) and **32 luminosity levels**.
- **Burn-in shift** range up to ±25 px.

Each run:

- reads the primary display's resolution and refresh rate, and caps the frame rate at the refresh rate;
- picks a random look: solids (shuffled shapes for the *Metrics* dialog's Auto rows), layout, static/orbit/focus/tilt/resonance camera, background luminosity, pattern and motion, colour
  gradient, wireframe and inverted fps (the corner legend is never shown);
- keeps rendering cheap: 1 sample per pixel, 1 bounce, adaptive quality on. A short render test
  measures this machine's speed for the chosen scene, and the text size (Consolas) is chosen so
  rendering at the full frame rate takes about 15% of the CPU (the CPU budget), then halved for
  double text density; full frames are spaced so rendering stays within that budget;
- draws the ASCII frames itself (fixed-width Consolas cells) in a borderless window covering the
  whole primary display at (0, 0), taskbar included, with the mouse pointer hidden, and ends on
  any key, click or mouse movement. No console window is involved.

**OLED burn-in protection** (on by default): every N frames, N random within a range (600–3600 by
default), the whole character grid moves 1 px horizontally and 1 px vertically, drifting within
±2 px of its centred position (settable up to ±25 px per axis), so no pixel shows the same edge for
long. Near the screen edges, the cells shifted past the edge are cut off and a black strip as wide
as the shift opens on the other side.

**Settings.** Screen Saver Settings › Settings… (`platomon.scr /c`) and `platomon-config.exe` open
the same dialog, run by a dialog procedure named `ScreenSaverConfigureDialog` as in the Win32
screensaver interface. **Random start**, the last check box of the *Appearance* group (on by
default), picks the look anew every run; turn it off to use the look chosen in the group instead:
layout, camera, colour gradient and source, background colour and pattern, wireframe, stats bar
and inverted frame rate, plus *Background options*'s background colour, luminosity, quasicrystal waves
and pattern flow (all greyed out while Random start is on). The **Randomize** button beside it rolls
random values into every *Appearance* setting, every *Background options* setting and every *Tuning* slider now, and turns Random
start off so they are kept (saved with OK or *Apply*). The rest of *Appearance* always applies,
Random start or not (Randomize still rolls it): halo colour, tag colour, *Tag positions*, *Tag size*, sound halo, halo bounce by frequency,
extended floor distance, the minimum floor distance, *Tags*, *Fixed tag length* and *Random font*. The *Performance & Features* group always applies too: the frame-rate cap (the
display's refresh rate by default), the CPU budget used to size the text, the *Rendering resolution*
with its *Manual* box, the number of luminosity levels, the palette size and dynamic camera distance, with
the OLED burn-in shift (on/off, frame interval, range) nested at its bottom. On/off settings are
check boxes, settings with three or more numeric steps are sliders showing their value (the main
dialog's are the CPU budget, luminosity levels, palette size and burn-in range; the rest are in
*Tuning*), and drop-downs show each option by its leading word, capitalised
(the settings file keeps the lowercase names).

**Layout and Rendering.** *Layout…* (beside *Tuning…*) opens the layout settings that apply every
run: *Ordering* (as listed, simple to complex, complex to simple, or random; see *Line and stack
layouts* above), *Orbit centre* (CPU, RAM, Network, Disk, Size or Spin) and the sliders *Spacing* (−2 to 2 on a doubling scale, centred on 0 = standard: each step of 1
doubles or halves the gaps, from a quarter to four times; orbits are 10% of their radius apart at 0,
2.5% to 40% across the range),
*Distance scaling* (−2 to 2, centred on 0 = off; see *Line and stack layouts* above), *Orbit velocity*, *Orbital inclination*, *Orbital eccentricity* and *Orbital variation* (see *Orbit
layout* above; the orbit settings only matter with the Orbit layout, and a random look rolls its own
orbit sliders unless they are set). *Rendering…* (in the frame-rate cap's row) opens the rendering
settings: first the *Renderer* drop-down (CPU, or Space heater on the GPU as characters or as
discrete pixels; below), then samples per cell, light bounces, march steps and render distance (auto:
the screensaver's own 1 sample and 1 bounce), adaptive quality and the **character ramp**: its *Custom ramp…* button
opens the *Character ramp* dialog, where *Use this custom ramp* swaps the luminosity levels' 8, 16 or
32 glyphs for a string of your own, darkest first (2 to 96 characters, no tabs; start with a space
for black). *Start from* fills in the 8, 16 or 32-level ramp to edit, and the line under the string
counts its glyphs or says what is wrong with it. The Rendering dialog shows whether it is on, and
the ramp is saved, on or off, with the other settings (`custom_ramp=on|off`, `ramp="..."`, quoted so
a leading space survives) and in log entries. Their sliders work like *Tuning*'s and are saved the
same way (`tune_spacing_exp`, `tune_distance_scale`, `tune_orbit_velocity`, `tune_spp`, ...); `ordering`, `orbit_center` and
`adaptive` are plain keys.

**Metrics.** *Metrics…* (between *Layout…* and *Preview*; it shows how many solids there are)
replaces the former *Extra metrics* drop-down. Its dialog has a row for each of up to 12 solids:
a check box to use the row, the *Shape* (Auto or one of the 15 solids), and drop-downs for what
drives its *Size*, *Spin*, *Colour* and *Complexity*, and a *Curvature* slider (-10 to +10, its value beside it)
with the drop-down that scales it (*Curvature by*: Off holds the slider's curvature, a metric takes the solid
from flat at idle to the slider's curvature at full load). The drop-downs offer Off, CPU, RAM, Disk, Network, GPU, Adapter
(the busiest network adapter), Disk dev (the busiest disk), CPU temp, GPU temp, CPU freq, GPU freq,
CPU volts or GPU volts, plus any named adapter, disk or core the settings already use. *Split CPU
into cores* (beside *Defaults*) turns each solid the whole CPU drives into one per logical core;
the button then reads *Metrics (5, cores)*. The **Δ** box beside each input uses the metric's change over the *Δ period*
(seconds, 1–600, below the rows) instead of its value; see *Per-solid inputs* above. *Defaults*
brings back the five standard solids (CPU, RAM, disk, network and GPU, each driving its own size,
spin and colour). The solids apply every run, Random start or not; an *Auto* shape takes the random
look's solid for its metric while that is free. They are saved as `solid1`..`solid12`
(`SHAPE|SIZE|SPIN|COLOUR|COMPLEXITY[|CURVATURE]`, unused ones unset) and `delta_period`; settings from before the
dialog turn `extras=nic|disk|both` into the five standard solids plus an adapter and/or a disk.

**Background options.** The *Background options…* button, in the *Appearance* group under its
drop-downs and above its check boxes, opens the background's settings (the pattern itself stays in
*Appearance*): *Background colour* (moved here from *Appearance*), *Background luminosity*,
*Quasicrystal waves* and *Pattern flow* (the speed of every moving pattern: quasicrystal, rain,
ocean, julia and clouds; Still holds them all; these four are part of the look, greyed out while
Random start is on), then the sliders moved here from *Tuning*: floor contrast, tile size (0.3 to 25), sway and
its period and floor turn period, and new sliders: floor scroll X and Y, and each pattern's own
settings: *Radial sectors*; for rain *Ripple spacing*, *Drop life* and *Raindrops* (how many drops
ripple at once, 1–48); for the ocean *Wind direction* and *Wave choppiness*; for julia *Julia shape*
and *Julia detail*; *Carpet levels*; for clouds *Cloud octaves* and *Cloud roughness*; for polka
*Dot size*, *Dot spacing* and *Dot variability* (0 = identical dots on an even triangular lattice, up to 1 = sizes
from none to twice the dot size and dots moved up to half the spacing), plus an *Invert half* check
box (part of the look, saved as `bg_invert_half`) that swaps light and dark on one side of a line
through the floor's centre, and for every pattern but plain a *Floor turn* drop-down (Off,
Clockwise, Anticlockwise; part of the look, saved as `bg_rotate`). **Only the
settings the chosen *Background pattern* uses are shown** (the dialog's title names it): the plain
floor shows just colour and luminosity, a still pattern has no flow, and only the quasicrystal shows
its waves. With *Random start* on, the pattern is random, so every setting is shown. Settings
hidden for the current pattern keep their values. The sliders work like *Tuning*'s
(`tune_bg_contrast`, ..., `tune_bg_drops`, `tune_bg_wind`, ...); the button shows how many of its
settings are set.

**Custom colour.** *Custom colour…*, beside *Background options…*, makes a colour gradient of your
own: 2 to 8 colours (*Colours*), darkest first, each typed as `#rrggbb` (or `#rgb`) or chosen with
*Pick…* (or a click on its swatch) in the Windows colour picker, with a preview strip of the
gradient below. *Generate from colour 1* fills the colours in for you, each dark to light: *Random*
(random hues), *Analogous* (hues next to colour 1's), *Complementary* (from colour 1's hue across
to its opposite), *Triadic* (colour 1's hue and the two a third of the wheel away) and *Shades*
(colour 1 from near black to near white); *Reverse* turns it round, and *Start from a preset*
*Load*s a built-in gradient to edit. *Use for* sets *Colour* (the solids) and/or *Background
colour* to *Custom*, which both drop-downs also offer. The gradient is saved as
`custom_gradient=#rrggbb:#rrggbb:...` and used, like any look setting, with Random start off.

**Tuning.** *Tuning…* opens a dialog of sliders: halo size swing (stepped like the main dialog's
sliders, saved as a plain key), then numeric values: orbit period and bob, focus zoom and glide,
smallest and largest solid size, idle and fastest spin, and exposure. Moving a slider sets that value for
every run (random look or not); *×* or *Reset all* returns it to auto (the random look's or the
default value). With Random start off, the floor keeps the defaults its *Background options*
sliders show on auto: it scrolls or sways only as those sliders are set, and turns only as
*Floor turn* says. Set values are saved as `tune_<name>` keys; the button shows how many are set.

**Preview.** *Preview* (beside *Tuning…*) shows the dialog's settings, saved or not, in a window
half the screen's size, rendered at half resolution with the full-screen run's text size. A click
inside the window closes it. With *Fullscreen* ticked it covers the whole screen at full
resolution instead; it ignores the mouse and closes on any key. While it runs, edits show live (the
dialog writes them to `preview.ini` beside the settings file, and the preview reads that a few times
a second, keeping its random look). Edits to the CPU budget, overdrive, rendering resolution and frame-rate cap set the
text size, which is fixed once the preview's window exists; after one of those, or after ticking or
unticking *Fullscreen*, the button reads *Restart* and starts a new preview. Closing the dialog
closes the preview. Run it directly with `platomon.scr /w` or `/w full`.

**Configuration log.** Each OK or *Apply* (which saves without closing) adds an entry to the log on
the right, titled with a random English word and the time, holding the settings and the arguments
the dialog was launched with. Entries always hold the look with *Random start* off, so
loading one brings back that look; a save whose settings are already logged adds no new entry. Pick an entry to see them in the box below the
list; the box can be edited, and *Load config* puts the settings it holds (its `key=value` lines
after `[platomon]`, as edited) back in the controls (saved only with OK). *Save conf*, beside it,
saves the box's settings to the log as a new entry instead (settings it leaves out take their
defaults), without touching the dialog's own settings. The newest 10 entries are kept; the `+` beside an entry keeps it
(the button turns to `−`, which lets it go again) and saves it to `configuration.log` beside the
settings file, and kept entries are never dropped. The `x` at an entry's right takes it off the list
without deleting it: it stays in the log file (marked `hidden=1`, and never dropped), and a kept
entry stays in `configuration.log` too; saving the same settings again puts it back on the list.
The log itself is `config-log.ini`.

**Defaults.** Defaults saved earlier with the (now removed) *Set as default* button stay in
`configuration.log`, and a first run with no settings file still starts from them.

**Space heater.** The Rendering dialog's *Renderer* drop-down picks what draws the picture: *CPU:
characters*, *GPU: Space heater, characters* or *Pixels: Space heater, discrete pixels* (INI
`renderer=cpu|gpu|pixels`; the older `space_heater` and `space_heater_pixels` keys are still read).
On the GPU it renders on the graphics card: a
Vulkan 1.1 compute shader (anything from a Radeon HD 7850, GCN 1.0, up) traces one character cell
per invocation and sends back the text stream, each cell's glyph, solid, luminance and depth; the
CPU only adds the halos, tags and wireframe and draws the window. On an RX 5700 XT a Boiler-mode
frame (1280 × 360 cells) takes about 5 ms against about 25 ms on all 16 threads of an i7-5960X.
The GPU covers the solids and every floor pattern, the animated ones included; PCs without Vulkan
1.1 render on the CPU as before. The text is still
sized by the CPU speed test, so tick *Manual* to use the extra speed for finer text
(`last-run-frames.txt` says how many frames the GPU drew). *Discrete pixels* goes further: one ray
per pixel of the display at any rendering resolution, drawn as discrete pixels instead of
characters, each the colour its glyph's ink would have scaled by its brightness, so the colour
settings still apply. The GPU colours the pixels itself and presents
them straight to the window through a Vulkan swapchain, so the picture never crosses to the CPU and
back: in a 1280 × 720 preview a frame costs about 3 ms, against about 15 ms when the pixels were read
back, coloured by the CPU and drawn with GDI. The stats bar and the F1 panel are drawn by the CPU and
handed to the GPU to lay over the picture. Full screen at 2560 × 1440 (3.7 million rays a frame) it
runs at about 70 fps on an RX 5700 XT: the CPU budget only counts CPU work, not the time spent
waiting for the GPU. Discrete pixels look like a conventionally rendered scene at the display's
native resolution, whatever the rendering resolution (which sizes only the text) and whichever floor;
the sound halo is drawn by the GPU too: each solid's halo edges as anti-aliased lines (wider on bigger screens) in the *Halo colour* chosen, hidden behind nearer surfaces like the text halo, and redrawn over the last traced picture between full frames at the display's refresh rate. Tags and the wireframe are not drawn on pixels yet.

**Rendering resolution.** The text is first sized so rendering fits the CPU budget, then divided:
Standard keeps the budget-sized text, Double (the default) halves its height, and Triple,
Quadruple, Quintuple and Boiler mode divide it by 3, 4, 5 and 6, never below 2 px. N times the
characters per axis costs about N² times the budget's rendering: Boiler mode is about 36x
(INI `density=1`..`6`). The **Manual** box beside it turns the automatic sizing off: the text size
then comes from the rendering resolution alone, 60 rows of text per step (Standard = 60 rows on the
screen, Double = 120, ... Boiler mode = 360; 2 to 40 px), whatever the speed test and CPU
budget say (INI `density_manual=on`). Expect more CPU load at the higher steps.

**Overdrive.** The check box beside the CPU budget slider ignores the budget: the text is sized for
the whole CPU at the full frame rate, and full frames render back to back with no pause between
them (up to the frame-rate cap). The slider is greyed out while it is on. Expect full CPU load, heat
and fan noise.

**Sound halo** (on by default). A second wireframe of every solid, 1.2x its size, follows the
sound level of the default output device (the one selected in Windows sound settings, read through
its WASAPI peak meter and followed when you switch devices): it grows to 1.3x the solid's radius
on loud passages and shrinks to 1.1x in silence (±10%, the default). **Halo size swing** can widen
that to ±30% (0.9x, inside the solid, to 1.5x) or ±60% (0.6x to 1.8x); tags keep clear of the
largest halo the swing allows. Its brightness follows the frequencies being
played: a loopback capture of the device is split by FFT into 12 bands (20 Hz – 16 kHz), weighted
so low frequencies count most (half weight at 150 Hz, falling further above), so bass makes the
halo bright and treble-only or silent passages leave it dim. Both size and brightness are smoothed
(fast rise, slower fall) and auto-gained over the last few seconds, so quiet material moves them
as well. The halo is hidden behind nearer solids.

**Halo colour** (with a colour gradient on). *Under* (the default) colours each halo cell like
whatever it passes over, a solid or the background. *Solid* gives each halo its own solid's colour
wherever it is drawn, over the background too. *Size* places it on the gradient by its size: quiet
(small) halos take the start of the gradient and loud (large) ones its end. With *Halo bounce by
frequency* each solid's halo has its own colour. *Bass* colours every halo by the bass level, which
also sets their brightness. Each colour is shaded by the halo's brightness like any other cell.

**Halo bounce by frequency** (on by default). Each solid's halo
size follows its own slice of the spectrum instead of the overall level. Solids are ranked by
complexity (edge count, faces breaking ties) and placed on a log gradient from the simplest in the
scene (the bass, 20 Hz) to the most complex (the treble, 16 kHz): with the five Platonic solids the
tetrahedron hears sub-bass, the cube and octahedron about 350 Hz, the dodecahedron and icosahedron
about 12 kHz. Each solid takes a Gaussian-weighted average of the bands around its place. The bands
are first tilted by √(f / 1 kHz) so typical music (falling about 3 dB per octave) moves every solid,
and each solid's auto-gain stays at least a tenth of the loudest solid's, so a bass-only sound leaves
the treble solids calm. The run log lists each solid's place and its size range and average.

The halo runs at the display's refresh rate (or the frame-rate cap) independently of the full
frames: between renders only the halo is recomputed, and only the cells it changes are redrawn
(about 0.5–0.7 ms per halo frame at 2560×1440). Full frames keep rendering within the CPU budget,
and halo-only frames fill the time between them.

Text is drawn from glyph images rendered once with GDI (ClearType) and tinted per colour, copied
straight into the back buffer, skipping cells that did not change; only the changed rows are copied
to the screen. Frame counts and per-stage timings of the last run are written to
`%LOCALAPPDATA%\platomon\screensaver\last-run-frames.txt`.

**Colour palettes.** Colour uses a fixed palette of 16 (default), 32, 64 or 128 colours sampled
from the gradient (*Palette size*, always applied). Besides brightness and load, the solids can be
coloured by their **spin** (speed and direction as seen from the camera) or by **sound**: each solid
by the loudness of its own frequency band, the simplest solids the bass and the most complex the
treble (this runs the output-device meter even with the halo off; a random look picks sound only
when the sound halo is on). *Background colour* maps the floor and sky independently of the solids:
by brightness through the solids' gradient, a gradient of its own, or uncoloured.

Because every colour is a palette entry, the tinted glyph tiles are few and stay cached. Measured at
640×240 cells with 16 solids over 90 animated frames (`cargo run --release --example color_bench`):
colouring by load used about 4,000 colours a frame with 24-bit colour and missed the tile cache 45
times a frame; with 16 colours it uses 16 and misses about 1.5 times. Together with a faster tile
cache (one packed key, one pixel arena), a single pass that picks each cell's glyph and colour, and a
table for the tone curve (within 2e-6 of the exact curve), the colour pipeline takes 2.7–3.0 ms
per frame (compose + draw) at 16 colours, down from 4.2–4.6 ms. Larger palettes cost almost nothing to
compose; colouring by brightness at 64 or 128 colours redraws more cells (finer steps change more
often), about +0.9–1.0 ms of drawing per frame.
**Tag size.** On small text (cells under 10 px tall) *Auto* switches tags to block letters drawn
with ordinary glyphs, which stay readable where single letters are only a few pixels high, and
draws them larger the finer the text: 1x from 5 px cells, 2x at 3–4 px and 3x below (so tags stay
roughly 15–30 px tall). *Tag size* (INI `tag_text`) can also keep one line always (`line`), or
always use block letters at a size of your choosing for fine pitch resolutions: *Blocks*, *Blocks
2x*, *Blocks 3x* or *Blocks 4x* (`blocks`, `blocks2`..`blocks4`). Each tag is a solid box that
hides what is behind it.

**Random font.** Block letters normally use the three-row stroke font (`_ | / \ ( )`; larger sizes
redraw each stroke over a k × k square). With *Random font* on (INI `tag_font_random=on`), each run
picks one of six fonts at random: the strokes, or five-row pixel letters drawn with `#`, `o`, `*`,
`@` or `[]` bricks (larger sizes fill each pixel's k × k square).

**Tag positions.** *Tag positions* (INI `tag_place`) chooses where the tags go:

- *Circle* (`circle`, the default): on a circle round the picture's centre, 80% of the picture's
  height across. Each tag heads for the point of the circle nearest its solid (straight out from the
  centre through it), so its leader is as short as the circle allows, and glides round toward it as
  the solids and the camera move. Neighbouring tags push each other apart along the circle so they
  never overlap, and the text runs outward, so the tags ring the picture with their leaders pointing in.
- *Beside* (`beside`): next to its solid on the diagonal toward the picture's nearest corner, as
  close as it can while staying clear of that solid (with its halo's room) and of every other solid.
- *Fixed corner* (`corner`): stacked in the picture's corner on its solid's side (its quarter of
  the picture), in the order of the solids' heights so leaders rarely cross.
- *Below* (`below`): centred under its solid, past its halo, with the leader running straight up;
  a tag that would sit on another steps down below it.
- *Trailing* (`trailing`): just past its solid's halo on the side away from the way the solid is
  moving across the picture, swinging round smoothly as the motion changes, like a trail.

*Fixed tag length* (in *Appearance*) applies to beside, below and trailing tags: on, they are
placed for each solid's current size, so the leader keeps one length as it grows and shrinks; off,
for the solid at full size, so they hold still. (Older settings with fixed tag length on and no
`tag_place` load as *Beside*.)

**Luminosity levels.** 8, 16 (default) or 32 glyphs from dark to bright, the same every run. The
8-level ramp `" .:-=+#@"` is the classic 10-level one without `*` and `%`. The 32-level ramp `` `-.':"~=*+c7ix5ah6HmB80$MWNQg&@`` (after a
blank) is the Consolas glyphs whose measured ink coverage is nearest to 32 even steps. Settings are stored, like the Windows SDK screensaver sample does, in a private
INI file through `WritePrivateProfileString`: `%APPDATA%\platomon\screensaver.ini`, section
`[platomon]`:

```ini
[platomon]
random=on              ; on = random look each run; off = use the look below
layout=rings           ; rings | sphere | line | stack | orbit
ordering=listed        ; listed | simple | complex | random
orbit_center=cpu       ; orbit layout's centre: cpu | ram | net | disk | size | spin
adaptive=on            ; adaptive quality
renderer=cpu           ; cpu | gpu (Space heater, characters) | pixels (Space heater, discrete pixels)
camera=orbit           ; static | orbit | focus | tilt | resonance
color=inferno          ; off | inferno | viridis | ... (see --list gradients) | custom
custom_gradient=#0b1d3a:#2f6db5:#7ad3e8:#f4f1c9 ; the Custom colour dialog's gradient, 2-8 colours
color_by=lum           ; lum | load | spin | sound
bg_color=auto          ; auto | off | match | custom | a gradient preset (background, by brightness)
colors=16              ; 16 | 32 | 64 | 128 (palette size; always applies)
bg_pattern=checker     ; solid | checker | diamond | brick | triangles | hex | radial | quasicrystal | rain | ocean | julia | carpet | clouds | polka
bg_lum=2               ; 0..10 (Background options)
bg_waves=7             ; quasicrystal waves (Background options)
bg_wave_speed=0.6      ; pattern flow, 0 = still (Background options)
tune_bg_drops=12       ; rain: raindrops at once (Background options, only when set)
bg_invert_half=off     ; polka: invert one side of a line through the centre (Background options)
bg_rotate=off          ; floor turn: off | cw | ccw (Background options)
tune_bg_dot_variability=0.5 ; polka: how much dot sizes and places vary, 0..1 (also tune_bg_dot_size, tune_bg_dot_spacing)
wireframe=off          ; on | off
stats=off              ; on | off
fps_invert=off         ; on | off
solid1=auto|cpu|cpu|cpu|off ; Metrics dialog: SHAPE|SIZE|SPIN|COLOUR|COMPLEXITY[|CURVATURE], up to solid12
solid3=auto|gpu|gpu|gpu|off|-6@cpu-temp ; curvature -10..10, fixed or VALUE@INPUT
solid2=cube|ram|delta:ram|ram|off ; delta:METRIC = its change over delta_period
delta_period=10        ; seconds, 1..600
split_cpu=off          ; on: one solid per logical core for each solid the whole CPU drives
fps_cap=display        ; or 30..240
budget=15              ; percent
overdrive=off          ; on = ignore the budget and use all the CPU time rendering wants
density=2              ; 1 = standard | 2 = double ... 6 = boiler mode
density_manual=off     ; on = size the text from the density alone (60 rows per step)
levels=16              ; 8 | 16 | 32 (older auto -> 16, 10 -> 8)
custom_ramp=off        ; on = draw with the ramp below instead of the levels' ramp
ramp=" .,:;-~=+*ox#%&@" ; custom character ramp, darkest first (quoted: keeps a leading space)
sound=on               ; sound halo from the default output device
halo_swing=10          ; halo size swing, percent of the radius: 10 | 30 | 60 (older halo_wide on -> 30)
halo_bands=on          ; on = each solid's halo follows its frequency band; off = overall level
halo_color=under       ; halo colour: under | solid | size | bass
tags=on                ; solid tags with leader lines
tag_color=complement   ; complement | theme | white | #RRGGBB (lifted to stay readable)
tag_text=auto          ; auto (line, or block letters 1x-3x under 10 px) | line | blocks | blocks2 | blocks3 | blocks4
auto_distance=off      ; dynamic camera distance (keep every solid in frame)
far_ground=off         ; extended floor distance
floor_clearance=on     ; minimum floor distance: solids float so their largest halo clears the floor
tag_fixed=off          ; beside/below/trailing tags: on = placed for the current size (fixed leader length)
tag_place=circle       ; circle | beside | corner | below | trailing
tag_font_random=off    ; on = a random block-letter font each run
tune_orbit_period=45   ; Tuning values (only those set): tune_<name>, see Tuning above
shift=on
shift_min_frames=600
shift_max_frames=3600
shift_radius=2         ; pixels, 1..25
```

The settings of the last run, with the measured speed and chosen font, are saved to
`%LOCALAPPDATA%\platomon\screensaver\last-run.toml`; `platomon --config` on that file reproduces it.
There is no miniature preview in the Screen Saver Settings window.

## Config file

All options have TOML keys (dashes become underscores); see `config.example.toml`. Solid and ring
choices go in tables. A kind-wide key like `nic` applies to every `nic:*` slot, and an exact key
wins over it. Command-line options override the file.

```toml
show = ["cpu", "ram", "nic:Ethernet", "nic:Wi-Fi"]
camera = "orbit"
aspect = "wxga"
bg_lum = 3.0
bg_pattern = "hex"
bg_scroll = [0.3, 0.0]
bg_rotate = "ccw"

[solids]
cpu = "truncated-icosahedron"
nic = "rhombic-dodecahedron"

[rings]
nic = 1

[weights]   # activity-score weight per metric kind
cpu = 2.0
```

## Build

```
cargo build --release                       # with NVIDIA NVML support (loaded at runtime)
cargo build --release --no-default-features # without
cargo test
cargo bench --bench render
```

## Notes

- **GPU backends**, in order: NVML (feature `nvidia`, no link-time dependency) → Linux sysfs
  `gpu_busy_percent` (AMD) → sysinfo `Gpus` (Windows PDH, macOS).
- **Feedback guard**: platomon's own CPU (per-core % ÷ cores) is subtracted from system CPU.
- **Activity score**: weighted mean over available metrics. For RAM it uses the rate of change, since
  a fill level isn't activity.
- **Adaptive quality**: when render time exceeds ~90% of the frame budget, spp drops to 1, then
  bounces to 1 (`q-1` / `q-2` in the stats bar); restored once there is headroom.
- **Polyhedra** are intersections of face planes. Vertices are recovered from the planes, and tests
  check face/vertex counts, unit circumradius and equal edge lengths for every solid.
- Every frame repaints the whole terminal area, margins included, so it doesn't depend on the terminal
  honouring clear-screen codes. Terminal size is polled each frame, for hosts that don't send resize
  events.
