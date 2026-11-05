//! sRGB, linear light and OKLab, plus palette quantization.

use std::sync::OnceLock;

use rayon::prelude::*;

use sshpdf_term::ColorDepth;
use sshpdf_term::palette::xterm_rgb;

/// A color in OKLab: lightness `L` (0..1) and the opponent axes `a`, `b`.
pub type Lab = [f32; 3];

fn srgb_to_linear_f(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_f(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB byte → linear light, by table.
pub fn srgb_to_linear(c: u8) -> f32 {
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0.0; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = srgb_to_linear_f(i as f32 / 255.0);
        }
        t
    })[c as usize]
}

/// Linear RGB → OKLab.
pub fn linear_to_lab(r: f32, g: f32, b: f32) -> Lab {
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.max(0.0).cbrt(), m.max(0.0).cbrt(), s.max(0.0).cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// sRGB bytes → OKLab.
pub fn rgb_to_lab(rgb: [u8; 3]) -> Lab {
    linear_to_lab(srgb_to_linear(rgb[0]), srgb_to_linear(rgb[1]), srgb_to_linear(rgb[2]))
}

/// OKLab → linear RGB (may be out of gamut).
pub fn lab_to_linear(lab: Lab) -> [f32; 3] {
    let [ll, a, b] = lab;
    let l = ll + 0.396_337_78 * a + 0.215_803_76 * b;
    let m = ll - 0.105_561_346 * a - 0.063_854_17 * b;
    let s = ll - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// OKLab → sRGB bytes, clamped to the gamut.
pub fn lab_to_rgb(lab: Lab) -> [u8; 3] {
    let lin = lab_to_linear(lab);
    lin.map(|c| (linear_to_srgb_f(c.clamp(0.0, 1.0)) * 255.0 + 0.5) as u8)
}

/// Squared distance between two OKLab colors.
pub fn dist2(a: Lab, b: Lab) -> f32 {
    let d0 = a[0] - b[0];
    let d1 = a[1] - b[1];
    let d2 = a[2] - b[2];
    d0 * d0 + d1 * d1 + d2 * d2
}

/// Chroma (colorfulness) of an OKLab color.
pub fn chroma(lab: Lab) -> f32 {
    (lab[1] * lab[1] + lab[2] * lab[2]).sqrt()
}

/// A palette with a nearest-color lookup.
pub struct Palette {
    /// Palette index of each entry (16-255 for the 256-color palette, 0-15
    /// for the 16-color one).
    pub index: Vec<u8>,
    pub lab: Vec<Lab>,
    /// Nearest entry for every color on a 64x64x64 RGB grid.
    lut: Vec<u8>,
}

impl Palette {
    fn build(indices: impl Iterator<Item = u8>) -> Palette {
        let index: Vec<u8> = indices.collect();
        let lab: Vec<Lab> = index.iter().map(|&i| rgb_to_lab(xterm_rgb(i))).collect();
        let mut lut = vec![0u8; 64 * 64 * 64];
        lut.par_chunks_mut(64 * 64).enumerate().for_each(|(r, chunk)| {
            for (n, slot) in chunk.iter_mut().enumerate() {
                let rgb = [r, n >> 6, n & 63].map(|v| (v * 255 / 63) as u8);
                let c = rgb_to_lab(rgb);
                let mut best = (f32::MAX, 0usize);
                for (k, p) in lab.iter().enumerate() {
                    let d = dist2(c, *p);
                    if d < best.0 {
                        best = (d, k);
                    }
                }
                *slot = best.1 as u8;
            }
        });
        Palette { index, lab, lut }
    }

    /// The palette for a color depth, or `None` for truecolor.
    pub fn for_depth(depth: ColorDepth) -> Option<&'static Palette> {
        static P256: OnceLock<Palette> = OnceLock::new();
        static P16: OnceLock<Palette> = OnceLock::new();
        match depth {
            ColorDepth::True => None,
            // The 6x6x6 cube and the gray ramp have fixed values; 0-15 are
            // themable, so they are left out.
            ColorDepth::Ansi256 => Some(P256.get_or_init(|| Palette::build(16..=255u8))),
            ColorDepth::Ansi16 => Some(P16.get_or_init(|| Palette::build(0..=15u8))),
        }
    }

    /// Position (in `self.index` / `self.lab`) of the entry nearest to `lab`.
    pub fn nearest(&self, lab: Lab) -> usize {
        let rgb = lab_to_rgb(lab);
        let q = |v: u8| (v as usize * 63 + 127) / 255;
        self.lut[q(rgb[0]) << 12 | q(rgb[1]) << 6 | q(rgb[2])] as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_reference_values() {
        // White is L=1, a=b=0; black is 0.
        let w = rgb_to_lab([255, 255, 255]);
        assert!((w[0] - 1.0).abs() < 1e-3 && w[1].abs() < 1e-3 && w[2].abs() < 1e-3);
        let k = rgb_to_lab([0, 0, 0]);
        assert!(k[0].abs() < 1e-6);
        // Pure sRGB red, from Björn Ottosson's reference: (0.6279, 0.2249, 0.1258).
        let r = rgb_to_lab([255, 0, 0]);
        assert!((r[0] - 0.6279).abs() < 1e-3, "{r:?}");
        assert!((r[1] - 0.2249).abs() < 1e-3, "{r:?}");
        assert!((r[2] - 0.1258).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn oklab_round_trip() {
        for rgb in [
            [0, 0, 0],
            [255, 255, 255],
            [12, 200, 99],
            [250, 3, 140],
            [128, 128, 128],
        ] {
            assert_eq!(lab_to_rgb(rgb_to_lab(rgb)), rgb);
        }
    }

    #[test]
    fn quantization_hits_exact_palette_colors() {
        let p = Palette::for_depth(ColorDepth::Ansi256).unwrap();
        for i in [16u8, 21, 196, 231, 232, 244, 255] {
            let k = p.nearest(rgb_to_lab(xterm_rgb(i)));
            assert_eq!(xterm_rgb(p.index[k]), xterm_rgb(i), "entry {i}");
        }
        let p16 = Palette::for_depth(ColorDepth::Ansi16).unwrap();
        assert_eq!(p16.index[p16.nearest(rgb_to_lab([250, 10, 10]))], 9);
        assert!(Palette::for_depth(ColorDepth::True).is_none());
    }

    #[test]
    fn grays_quantize_to_grays() {
        let p = Palette::for_depth(ColorDepth::Ansi256).unwrap();
        for v in (0..=255).step_by(17) {
            let rgb = xterm_rgb(p.index[p.nearest(rgb_to_lab([v, v, v]))]);
            assert!(rgb[0] == rgb[1] && rgb[1] == rgb[2], "{v} -> {rgb:?}");
        }
    }
}
