//! Glyph-tile cache for pixel output: each (glyph, colour) pair's pixels,
//! tinted once from the glyph's ink coverage and reused. With a fixed
//! palette the number of pairs is small, so after warm-up every cell is a
//! hit: one hash of a packed 64-bit key into one contiguous pixel arena.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Multiply-shift hash for packed `u64` keys (the default SipHash is several
/// times slower and needs no DoS resistance here).
#[derive(Default, Clone, Copy)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }

    fn write_u64(&mut self, x: u64) {
        let h = (self.0 ^ x).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = h ^ (h >> 29);
    }

    fn write_u16(&mut self, x: u16) {
        self.write_u64(x as u64);
    }
}

type FastMap<V> = HashMap<u64, V, BuildHasherDefault<KeyHasher>>;

/// Tinted glyph tiles of `px` pixels (`0x00RRGGBB`), bounded: when
/// `max_tiles` are held the cache starts over.
pub struct TileCache {
    px: usize,
    max_tiles: usize,
    /// Glyph → offset of its coverage in `coverage`.
    glyphs: FastMap<usize>,
    coverage: Vec<u32>,
    /// (glyph, colour) → offset of its tinted tile in `arena`.
    tinted: FastMap<usize>,
    arena: Vec<u32>,
    /// Tiles tinted so far (cache misses).
    pub misses: u64,
}

impl TileCache {
    pub const MAX_TILES: usize = 8192;

    pub fn new(px: usize) -> Self {
        Self::with_capacity(px, Self::MAX_TILES)
    }

    pub fn with_capacity(px: usize, max_tiles: usize) -> Self {
        Self {
            px,
            max_tiles: max_tiles.max(1),
            glyphs: FastMap::default(),
            coverage: Vec::new(),
            tinted: FastMap::default(),
            arena: Vec::new(),
            misses: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.tinted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tinted.is_empty()
    }

    /// The pixels of glyph `u` in colour `rgb`; `render(u)` gives a glyph's
    /// per-channel ink coverage (white on black, `px` pixels) the first time
    /// it is seen.
    #[inline]
    pub fn tile(&mut self, u: u16, rgb: u32, render: impl FnOnce(u16) -> Vec<u32>) -> &[u32] {
        let key = ((u as u64) << 32) | (rgb & 0x00FF_FFFF) as u64;
        let at = match self.tinted.get(&key) {
            Some(&at) => at,
            None => self.tint(u, rgb, key, render),
        };
        &self.arena[at..at + self.px]
    }

    #[cold]
    fn tint(&mut self, u: u16, rgb: u32, key: u64, render: impl FnOnce(u16) -> Vec<u32>) -> usize {
        self.misses += 1;
        let px = self.px;
        let cov = match self.glyphs.get(&(u as u64)) {
            Some(&c) => c,
            None => {
                let mut c = render(u);
                c.resize(px, 0);
                let at = self.coverage.len();
                self.coverage.extend(c);
                self.glyphs.insert(u as u64, at);
                at
            }
        };
        if self.tinted.len() >= self.max_tiles {
            self.tinted.clear();
            self.arena.clear();
        }
        let (cr, cg, cb) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
        let at = self.arena.len();
        for k in 0..px {
            let p = self.coverage[cov + k];
            let (r, g, b) = ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
            self.arena.push((((r * cr + 127) / 255) << 16) | (((g * cg + 127) / 255) << 8) | ((b * cb + 127) / 255));
        }
        self.tinted.insert(key, at);
        at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_are_tinted_once_and_bounded() {
        let mut renders = 0;
        let mut c = TileCache::with_capacity(4, 3);
        let mut cov = |_| {
            renders += 1;
            vec![0x00FF_FFFF, 0x0080_8080, 0, 0x00FF_0000]
        };
        assert_eq!(c.tile(b'a' as u16, 0x00FF_8000, &mut cov), &[0xFF8000, 0x804000, 0, 0xFF0000]);
        assert_eq!(c.tile(b'a' as u16, 0x00FF_8000, &mut cov), &[0xFF8000, 0x804000, 0, 0xFF0000]);
        assert_eq!((c.misses, c.len()), (1, 1));
        c.tile(b'a' as u16, 0x0000_00FF, &mut cov);
        c.tile(b'b' as u16, 0x0000_00FF, &mut cov);
        assert_eq!((c.misses, c.len()), (3, 3));
        // full: starts over, coverage kept
        assert_eq!(c.tile(b'c' as u16, 0x00FF_FFFF, &mut cov), &[0xFFFFFF, 0x808080, 0, 0xFF0000]);
        assert_eq!(c.len(), 1);
        assert_eq!(c.tile(b'a' as u16, 0x00FF_8000, &mut cov)[1], 0x804000);
        drop(cov);
        assert_eq!(renders, 3, "coverage rendered once per glyph");
    }
}
