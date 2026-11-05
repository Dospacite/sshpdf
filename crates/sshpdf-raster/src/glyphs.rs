//! Glyph sets: characters with the part of the cell they cover.
//!
//! A mask holds the coverage (0..1) of each sample of the sampling grid,
//! [`SW`] x [`SH`] samples per cell, row by row.

use std::sync::OnceLock;

use sshpdf_term::Tier;

include!(concat!(env!("OUT_DIR"), "/glyph_tables.rs"));

/// Samples per cell, horizontally.
pub const SW: usize = 8;
/// Samples per cell, vertically.
pub const SH: usize = 16;
/// Samples per cell.
pub const N: usize = SW * SH;

pub type Mask = [f32; N];

/// A character and its coverage mask.
#[derive(Clone, Debug)]
pub struct Glyph {
    pub ch: char,
    pub mask: Mask,
}

/// The coverage of each sample by the rectangle `[x0,x1) x [y0,y1)` given
/// in cell units (0..1, y down), by exact area.
pub fn rect_mask(x0: f32, y0: f32, x1: f32, y1: f32) -> Mask {
    let mut m = [0.0; N];
    for sy in 0..SH {
        let (a0, a1) = (sy as f32 / SH as f32, (sy + 1) as f32 / SH as f32);
        let oy = (a1.min(y1) - a0.max(y0)).max(0.0) * SH as f32;
        if oy <= 0.0 {
            continue;
        }
        for sx in 0..SW {
            let (b0, b1) = (sx as f32 / SW as f32, (sx + 1) as f32 / SW as f32);
            let ox = (b1.min(x1) - b0.max(x0)).max(0.0) * SW as f32;
            m[sy * SW + sx] = ox * oy;
        }
    }
    m
}

fn union(a: &Mask, b: &Mask) -> Mask {
    let mut m = *a;
    for (x, y) in m.iter_mut().zip(b) {
        *x = (*x + *y).min(1.0);
    }
    m
}

fn uniform(v: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> Mask {
    rect_mask(x0, y0, x1, y1).map(|c| c * v)
}

/// Coverage of a region given by an inside-test, by 4x4 supersampling.
fn shape_mask(inside: impl Fn(f32, f32) -> bool) -> Mask {
    let mut m = [0.0; N];
    for sy in 0..SH {
        for sx in 0..SW {
            let mut hits = 0;
            for j in 0..4 {
                for i in 0..4 {
                    let x = (sx as f32 + (i as f32 + 0.5) / 4.0) / SW as f32;
                    let y = (sy as f32 + (j as f32 + 0.5) / 4.0) / SH as f32;
                    if inside(x, y) {
                        hits += 1;
                    }
                }
            }
            m[sy * SW + sx] = hits as f32 / 16.0;
        }
    }
    m
}

/// Mask of an octant pattern (bit `n - 1` = position `n`, row by row).
pub fn octant_mask(pattern: u8) -> Mask {
    let mut m = [0.0; N];
    for p in 0..8 {
        if pattern >> p & 1 == 1 {
            let (row, col) = ((p / 2) as f32, (p % 2) as f32);
            m = union(
                &m,
                &rect_mask(col * 0.5, row * 0.25, col * 0.5 + 0.5, row * 0.25 + 0.25),
            );
        }
    }
    m
}

/// Mask of a sextant pattern (bit `n - 1` = position `n`, row by row).
pub fn sextant_mask(pattern: u8) -> Mask {
    let mut m = [0.0; N];
    for p in 0..6 {
        if pattern >> p & 1 == 1 {
            let (row, col) = ((p / 2) as f32, (p % 2) as f32);
            m = union(&m, &rect_mask(col * 0.5, row / 3.0, col * 0.5 + 0.5, (row + 1.0) / 3.0));
        }
    }
    m
}

/// Braille dot `d` (0-7, Unicode bit order) as (column, row).
fn braille_dot(d: usize) -> (f32, f32) {
    match d {
        0..=2 => (0.0, d as f32),
        3..=5 => (1.0, (d - 3) as f32),
        6 => (0.0, 3.0),
        _ => (1.0, 3.0),
    }
}

fn braille_mask(bits: u8) -> Mask {
    let mut m = [0.0; N];
    for d in 0..8 {
        if bits >> d & 1 == 1 {
            let (c, r) = braille_dot(d);
            // A dot is about a quarter of the cell wide, centred in its
            // octant: the gaps between dots are why braille is line-art only.
            let x0 = c * 0.5 + 0.125;
            let y0 = r * 0.25 + 0.0625;
            m = union(&m, &rect_mask(x0, y0, x0 + 0.25, y0 + 0.125));
        }
    }
    m
}

fn ascii_glyphs() -> Vec<Glyph> {
    let r = rect_mask;
    let g = |ch, mask| Glyph { ch, mask };
    vec![
        g(' ', [0.0; N]),
        g('.', r(0.375, 0.75, 0.625, 0.875)),
        g(
            ':',
            union(&r(0.375, 0.25, 0.625, 0.375), &r(0.375, 0.6875, 0.625, 0.8125)),
        ),
        g('-', r(0.125, 0.4375, 0.875, 0.5625)),
        g(
            '=',
            union(&r(0.125, 0.3125, 0.875, 0.4375), &r(0.125, 0.5625, 0.875, 0.6875)),
        ),
        g(
            '+',
            union(&r(0.125, 0.4375, 0.875, 0.5625), &r(0.4375, 0.25, 0.5625, 0.75)),
        ),
        g('*', uniform(0.3, 0.125, 0.25, 0.875, 0.75)),
        g(
            '#',
            [
                r(0.25, 0.1875, 0.375, 0.8125),
                r(0.625, 0.1875, 0.75, 0.8125),
                r(0.125, 0.375, 0.875, 0.4375),
                r(0.125, 0.5625, 0.875, 0.625),
            ]
            .iter()
            .fold([0.0; N], |a, b| union(&a, b)),
        ),
        g('%', uniform(0.35, 0.0, 0.125, 1.0, 0.875)),
        g('@', uniform(0.55, 0.0625, 0.1875, 0.9375, 0.875)),
    ]
}

/// Legacy Computing wedges and triangles, shade blocks and eighth blocks.
fn shape_glyphs() -> Vec<Glyph> {
    let mut v = Vec::new();
    for &(ch, a, b, c) in WEDGES {
        let side = |x: f32, y: f32| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
        let want = side(c[0], c[1]) > 0.0;
        v.push(Glyph {
            ch,
            mask: shape_mask(|x, y| (side(x, y) > 0.0) == want),
        });
    }
    // Triangles from the cell centre to two corners.
    let tri = |p: [(f32, f32); 3]| {
        move |x: f32, y: f32| {
            let s = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
            let (d0, d1, d2) = (s(p[0], p[1]), s(p[1], p[2]), s(p[2], p[0]));
            (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
        }
    };
    let ctr = (0.5, 0.5);
    let left = tri([(0.0, 0.0), (0.0, 1.0), ctr]);
    let upper = tri([(0.0, 0.0), (1.0, 0.0), ctr]);
    let right = tri([(1.0, 0.0), (1.0, 1.0), ctr]);
    let lower = tri([(0.0, 1.0), (1.0, 1.0), ctr]);
    v.push(Glyph {
        ch: '\u{1FB68}',
        mask: shape_mask(|x, y| !left(x, y)),
    });
    v.push(Glyph {
        ch: '\u{1FB69}',
        mask: shape_mask(|x, y| !upper(x, y)),
    });
    v.push(Glyph {
        ch: '\u{1FB6A}',
        mask: shape_mask(|x, y| !right(x, y)),
    });
    v.push(Glyph {
        ch: '\u{1FB6B}',
        mask: shape_mask(|x, y| !lower(x, y)),
    });
    v.push(Glyph {
        ch: '\u{1FB6C}',
        mask: shape_mask(left),
    });
    v.push(Glyph {
        ch: '\u{1FB6D}',
        mask: shape_mask(upper),
    });
    v.push(Glyph {
        ch: '\u{1FB6E}',
        mask: shape_mask(right),
    });
    v.push(Glyph {
        ch: '\u{1FB6F}',
        mask: shape_mask(lower),
    });
    // Shades.
    for (ch, d) in [('░', 0.25), ('▒', 0.5), ('▓', 0.75)] {
        v.push(Glyph { ch, mask: [d; N] });
    }
    // Eighth blocks.
    for (k, ch) in ['▁', '▂', '▃', '▄', '▅', '▆', '▇'].into_iter().enumerate() {
        let h = (k + 1) as f32 / 8.0;
        v.push(Glyph {
            ch,
            mask: rect_mask(0.0, 1.0 - h, 1.0, 1.0),
        });
    }
    for (k, ch) in ['▏', '▎', '▍', '▌', '▋', '▊', '▉'].into_iter().enumerate() {
        let w = (k + 1) as f32 / 8.0;
        v.push(Glyph {
            ch,
            mask: rect_mask(0.0, 0.0, w, 1.0),
        });
    }
    v.push(Glyph {
        ch: '▔',
        mask: rect_mask(0.0, 0.0, 1.0, 0.125),
    });
    v.push(Glyph {
        ch: '▕',
        mask: rect_mask(0.875, 0.0, 1.0, 1.0),
    });
    v
}

fn base_glyphs(tier: Tier) -> Vec<Glyph> {
    match tier {
        Tier::Ascii => ascii_glyphs(),
        Tier::Half => [0u8, 0b0000_1111, 0b1111_0000, 0xFF]
            .into_iter()
            .map(|p| Glyph {
                ch: OCTANTS[p as usize],
                mask: octant_mask(p),
            })
            .collect(),
        Tier::Quadrant => (0u8..16)
            .map(|q| {
                // Quadrant bits UL, UR, LL, LR -> octant positions.
                let mut p = 0u8;
                for (bit, oct) in [(1, 0b0000_0101), (2, 0b0000_1010), (4, 0b0101_0000), (8, 0b1010_0000)] {
                    if q & bit != 0 {
                        p |= oct;
                    }
                }
                Glyph {
                    ch: OCTANTS[p as usize],
                    mask: octant_mask(p),
                }
            })
            .collect(),
        Tier::Sextant => (0u8..64)
            .map(|p| Glyph {
                ch: SEXTANTS[p as usize],
                mask: sextant_mask(p),
            })
            .collect(),
        Tier::Octant => (0..=255u8)
            .map(|p| Glyph {
                ch: OCTANTS[p as usize],
                mask: octant_mask(p),
            })
            .collect(),
        Tier::Braille => (0..=255u8)
            .map(|b| Glyph {
                ch: char::from_u32(0x2800 + b as u32).expect("braille"),
                mask: braille_mask(b),
            })
            .collect(),
    }
}

/// Blur a mask with a 3-tap Gaussian (σ = 0.5 sample), clamping at the
/// cell edges (the neighbours of a candidate are unknown).
pub fn blur_mask(m: &Mask) -> Mask {
    const W1: f32 = 0.135_335_28; // exp(-2): weight of a neighbour at σ = 0.5
    let pass = |src: &Mask, dx: usize, dy: usize| {
        let mut out = [0.0; N];
        for y in 0..SH {
            for x in 0..SW {
                let mut sum = src[y * SW + x];
                let mut wsum = 1.0;
                if (dx == 1 && x > 0) || (dy == 1 && y > 0) {
                    sum += W1 * src[(y - dy) * SW + x - dx];
                    wsum += W1;
                }
                if (dx == 1 && x + 1 < SW) || (dy == 1 && y + 1 < SH) {
                    sum += W1 * src[(y + dy) * SW + x + dx];
                    wsum += W1;
                }
                out[y * SW + x] = sum / wsum;
            }
        }
        out
    };
    pass(&pass(m, 1, 0), 0, 1)
}

/// A glyph set with the per-glyph sums the optimizer needs.
pub struct GlyphSet {
    pub tier: Tier,
    pub shapes: bool,
    pub glyphs: Vec<Glyph>,
    /// Blurred masks.
    pub(crate) blurred: Vec<Mask>,
    /// Σ m
    pub(crate) sum: Vec<f32>,
    /// Σ m_blurred
    pub(crate) sum_b: Vec<f32>,
    /// Σ m_blurred²
    pub(crate) sum_b2: Vec<f32>,
    complement: Vec<Option<u16>>,
    /// Index of the empty glyph.
    blank: usize,
}

fn same_mask(a: &Mask, b: &Mask) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

impl GlyphSet {
    /// Build a glyph set. `shapes` adds the shape glyphs to tiers ≥ sextant.
    pub fn new(tier: Tier, shapes: bool) -> GlyphSet {
        let shapes = shapes && matches!(tier, Tier::Sextant | Tier::Octant);
        let mut glyphs: Vec<Glyph> = Vec::new();
        let extra = if shapes { shape_glyphs() } else { Vec::new() };
        for g in base_glyphs(tier).into_iter().chain(extra) {
            if !glyphs.iter().any(|h| same_mask(&h.mask, &g.mask)) {
                glyphs.push(g);
            }
        }
        let blurred: Vec<Mask> = glyphs.iter().map(|g| blur_mask(&g.mask)).collect();
        let sum: Vec<f32> = glyphs.iter().map(|g| g.mask.iter().sum()).collect();
        let sum_b = blurred.iter().map(|m| m.iter().sum()).collect();
        let sum_b2 = blurred.iter().map(|m| m.iter().map(|v| v * v).sum()).collect();
        let complement = glyphs
            .iter()
            .map(|g| {
                let inv = g.mask.map(|v| 1.0 - v);
                glyphs.iter().position(|h| same_mask(&h.mask, &inv)).map(|i| i as u16)
            })
            .collect();
        let blank = sum
            .iter()
            .position(|s| *s < 1e-6)
            .expect("every set has an empty glyph");
        GlyphSet {
            tier,
            shapes,
            glyphs,
            blurred,
            sum,
            sum_b,
            sum_b2,
            complement,
            blank,
        }
    }

    /// A shared, lazily built glyph set.
    pub fn get(tier: Tier, shapes: bool) -> &'static GlyphSet {
        static SETS: [[OnceLock<GlyphSet>; 2]; 6] = [const { [const { OnceLock::new() }; 2] }; 6];
        let shapes = shapes && matches!(tier, Tier::Sextant | Tier::Octant);
        let t = Tier::ALL.iter().position(|t| *t == tier).expect("known tier");
        SETS[t][shapes as usize].get_or_init(|| GlyphSet::new(tier, shapes))
    }

    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// The glyph covering exactly the part `i` leaves free, if the set has it.
    pub fn complement(&self, i: usize) -> Option<usize> {
        self.complement[i].map(usize::from)
    }

    /// Fraction of the cell covered by glyph `i`.
    pub fn coverage(&self, i: usize) -> f32 {
        self.sum[i] / N as f32
    }

    /// Index of the space glyph (empty mask).
    pub fn blank_index(&self) -> usize {
        self.blank
    }
}

/// The coverage mask of any non-ASCII glyph sshpdf draws in the raster
/// layer (blocks, quadrants, sextants, octants, braille, shapes), for
/// drawing cells without a terminal.
pub fn mask_for(ch: char) -> Option<&'static Mask> {
    use std::collections::HashMap;
    static MAP: OnceLock<HashMap<char, Mask>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        for (tier, shapes) in [(Tier::Octant, true), (Tier::Sextant, true), (Tier::Braille, false)] {
            for g in GlyphSet::get(tier, shapes).glyphs.iter() {
                if !g.ch.is_ascii() {
                    map.entry(g.ch).or_insert(g.mask);
                }
            }
        }
        // The raster layer never picks these, but probe cards show them.
        for g in shape_glyphs() {
            map.entry(g.ch).or_insert(g.mask);
        }
        map
    })
    .get(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_256_octant_patterns_exactly_once() {
        let chars: HashSet<char> = OCTANTS.iter().copied().collect();
        assert_eq!(chars.len(), 256, "a character is used for two patterns");
        // Spot checks against the Unicode names.
        assert_eq!(OCTANTS[0], ' ');
        assert_eq!(OCTANTS[0xFF], '█');
        assert_eq!(OCTANTS[0b0000_1111], '▀');
        assert_eq!(OCTANTS[0b0101_0101], '▌');
        assert_eq!(OCTANTS[0b0000_0100], '\u{1CD00}'); // BLOCK OCTANT-3
        assert_eq!(OCTANTS[0b0000_0110], '\u{1CD01}'); // BLOCK OCTANT-23
        assert_eq!(OCTANTS[0b1111_1110], '\u{1CDE5}'); // BLOCK OCTANT-2345678
        let set = GlyphSet::new(Tier::Octant, false);
        assert_eq!(set.len(), 256);
    }

    #[test]
    fn all_64_sextant_patterns_exactly_once() {
        let chars: HashSet<char> = SEXTANTS.iter().copied().collect();
        assert_eq!(chars.len(), 64);
        assert_eq!(SEXTANTS[0b000001], '\u{1FB00}'); // BLOCK SEXTANT-1
        assert_eq!(SEXTANTS[0b010101], '▌');
        assert_eq!(SEXTANTS[0b111110], '\u{1FB3B}'); // BLOCK SEXTANT-23456
        assert_eq!(GlyphSet::new(Tier::Sextant, false).len(), 64);
    }

    #[test]
    fn octant_masks_tile_the_cell() {
        let total: f32 = (0..8).map(|p| octant_mask(1 << p).iter().sum::<f32>()).sum();
        assert!((total - N as f32).abs() < 1e-3);
        // Position 3 is row 1, column 0: samples x 0..4, y 4..8.
        let m = octant_mask(0b100);
        assert_eq!(m[4 * SW], 1.0);
        assert_eq!(m[4 * SW + 4], 0.0);
        assert_eq!(m[3 * SW], 0.0);
    }

    #[test]
    fn sextant_rows_are_thirds() {
        let m = sextant_mask(0b000011); // top row
        let s: f32 = m.iter().sum();
        assert!((s - N as f32 / 3.0).abs() < 1e-3);
    }

    #[test]
    fn tier_sizes_and_complements() {
        for (tier, n) in [(Tier::Half, 4), (Tier::Quadrant, 16), (Tier::Braille, 256)] {
            assert_eq!(GlyphSet::new(tier, false).len(), n, "{tier}");
        }
        let q = GlyphSet::new(Tier::Quadrant, false);
        let upper = q.glyphs.iter().position(|g| g.ch == '▀').unwrap();
        assert_eq!(q.glyphs[q.complement(upper).unwrap()].ch, '▄');
        let b = GlyphSet::new(Tier::Braille, false);
        assert!(b.complement(1).is_none());
    }

    #[test]
    fn shapes_are_added_only_to_fine_tiers() {
        let s = GlyphSet::new(Tier::Sextant, true);
        assert!(s.len() > 64 + 44, "{}", s.len());
        assert!(s.glyphs.iter().any(|g| g.ch == '\u{1FB3C}'));
        assert_eq!(GlyphSet::new(Tier::Quadrant, true).len(), 16);
        // Wedge U+1FB3C fills the lower-left corner but not the upper right.
        let w = &s.glyphs.iter().find(|g| g.ch == '\u{1FB3C}').unwrap().mask;
        assert!(w[(SH - 1) * SW] > 0.9);
        assert_eq!(w[SW - 1], 0.0);
    }

    #[test]
    fn blur_keeps_mass_roughly_and_bounds() {
        let m = octant_mask(0b0000_0011);
        let b = blur_mask(&m);
        assert!(b.iter().all(|v| (0.0..=1.0).contains(v)));
        let (s0, s1): (f32, f32) = (m.iter().sum(), b.iter().sum());
        assert!((s0 - s1).abs() / s0 < 0.1, "{s0} {s1}");
    }
}
