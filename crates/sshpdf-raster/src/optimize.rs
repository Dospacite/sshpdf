//! The per-cell optimizer: for each cell, pick the glyph and the two colors
//! that best reproduce the rendered samples.
//!
//! For a candidate mask `m`, the best colors are the means of the covered and
//! uncovered samples. The error is the squared OKLab difference between the
//! source and the candidate, both blurred slightly (σ = 0.5 sample) so thin
//! lines count even when they don't fill a sub-cell.
//!
//! Every sum the error needs is a dot product of the mask (or blurred mask)
//! with the samples, so a cell costs `glyphs × samples` multiply-adds. With
//! colors limited to a palette, the best few candidates are scored again
//! with quantized colors, and the cell's color error is diffused to its
//! neighbours (Floyd–Steinberg weights at cell level).

use sshpdf_term::ColorDepth;
use rayon::prelude::*;

use crate::color::{Lab, Palette, linear_to_lab, srgb_to_linear};
use crate::glyphs::{GlyphSet, Mask, N, SH, SW};

/// The chosen glyph and colors for a cell (colors in OKLab, already
/// quantized to the palette when the depth is below truecolor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterCell {
    pub glyph: u16,
    pub fg: Lab,
    pub bg: Lab,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub depth: ColorDepth,
    /// Diffuse each cell's color error to its neighbours (only has an effect
    /// with a palette; in truecolor the cell mean is exact).
    pub diffusion: bool,
    /// Compare blurred source and candidates.
    pub blur: bool,
    /// Colors added to the palette when quantizing (the terminal's assumed
    /// default background and foreground).
    pub extras: [Option<Lab>; 2],
    /// In truecolor: reuse the previous cell's color when the difference is
    /// below this OKLab distance (fewer color changes to send; 0 = off).
    pub coherence: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            depth: ColorDepth::True,
            diffusion: true,
            blur: true,
            extras: [None, None],
            coherence: 0.0,
        }
    }
}

/// Make neighbouring cells along a row share colors that differ by less
/// than `tol`: a terminal row is written left to right, and an unchanged
/// color costs no bytes. Below the just-noticeable difference (about 0.02 in
/// OKLab) this doesn't show, but halves the output of photos.
fn cohere(cells: &mut [RasterCell], cols: usize, tol: f32) {
    let t2 = tol * tol;
    for row in cells.chunks_mut(cols.max(1)) {
        let mut fg: Option<Lab> = None;
        let mut bg: Option<Lab> = None;
        for c in row.iter_mut() {
            for (cur, prev) in [(&mut c.fg, &mut fg), (&mut c.bg, &mut bg)] {
                match prev {
                    Some(p) if crate::color::dist2(*cur, *p) < t2 => *cur = *p,
                    _ => *prev = Some(*cur),
                }
            }
        }
    }
}

/// Nearest color of a palette plus a few extra colors.
struct Quantizer<'p> {
    palette: &'p Palette,
    extras: [Option<Lab>; 2],
}

impl Quantizer<'_> {
    fn quantize(&self, lab: Lab) -> Lab {
        let mut best = self.palette.lab[self.palette.nearest(lab)];
        let mut d = crate::color::dist2(best, lab);
        for e in self.extras.iter().flatten() {
            let de = crate::color::dist2(*e, lab);
            if de < d {
                d = de;
                best = *e;
            }
        }
        best
    }
}

/// An image in OKLab, one plane per channel.
pub struct LabImage {
    pub width: usize,
    pub height: usize,
    pub planes: [Vec<f32>; 3],
}

impl LabImage {
    /// Convert RGBA8 pixels (alpha ignored; render over an opaque background).
    pub fn from_rgba(rgba: &[u8], width: usize, height: usize) -> LabImage {
        assert_eq!(rgba.len(), width * height * 4);
        let mut planes = [
            vec![0.0; width * height],
            vec![0.0; width * height],
            vec![0.0; width * height],
        ];
        let [p0, p1, p2] = &mut planes;
        p0.par_chunks_mut(width)
            .zip(p1.par_chunks_mut(width))
            .zip(p2.par_chunks_mut(width))
            .zip(rgba.par_chunks(width * 4))
            .for_each(|(((l, a), b), row)| {
                // Cache the last conversion: rendered pages have long runs
                // of one color (paper).
                let mut last = ([0u8; 3], [0.0f32; 3]);
                let mut first = true;
                for x in 0..width {
                    let px = [row[x * 4], row[x * 4 + 1], row[x * 4 + 2]];
                    if first || px != last.0 {
                        let lab = linear_to_lab(srgb_to_linear(px[0]), srgb_to_linear(px[1]), srgb_to_linear(px[2]));
                        last = (px, lab);
                        first = false;
                    }
                    l[x] = last.1[0];
                    a[x] = last.1[1];
                    b[x] = last.1[2];
                }
            });
        LabImage { width, height, planes }
    }

    /// Blur with the same 3-tap Gaussian as the masks (σ = 0.5 sample),
    /// clamping at the image edges.
    pub fn blurred(&self) -> LabImage {
        const W1: f32 = 0.135_335_28;
        let (w, h) = (self.width, self.height);
        let planes = self.planes.each_ref().map(|src| {
            let mut tmp = vec![0.0; w * h];
            tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let s = &src[y * w..(y + 1) * w];
                for x in 0..w {
                    let mut sum = s[x];
                    let mut ws = 1.0;
                    if x > 0 {
                        sum += W1 * s[x - 1];
                        ws += W1;
                    }
                    if x + 1 < w {
                        sum += W1 * s[x + 1];
                        ws += W1;
                    }
                    row[x] = sum / ws;
                }
            });
            let mut out = vec![0.0; w * h];
            out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for x in 0..w {
                    let mut sum = tmp[y * w + x];
                    let mut ws = 1.0;
                    if y > 0 {
                        sum += W1 * tmp[(y - 1) * w + x];
                        ws += W1;
                    }
                    if y + 1 < h {
                        sum += W1 * tmp[(y + 1) * w + x];
                        ws += W1;
                    }
                    row[x] = sum / ws;
                }
            });
            out
        });
        LabImage {
            width: w,
            height: h,
            planes,
        }
    }

    /// Replace each pixel's color by `f(x, y, color)`.
    pub fn map(&mut self, f: impl Fn(usize, usize, Lab) -> Lab + Sync) {
        let w = self.width;
        let [p0, p1, p2] = &mut self.planes;
        p0.par_chunks_mut(w)
            .zip(p1.par_chunks_mut(w))
            .zip(p2.par_chunks_mut(w))
            .enumerate()
            .for_each(|(y, ((l, a), b))| {
                for x in 0..w {
                    let c = f(x, y, [l[x], a[x], b[x]]);
                    l[x] = c[0];
                    a[x] = c[1];
                    b[x] = c[2];
                }
            });
    }

    fn gather(&self, col: usize, row: usize, out: &mut [[f32; N]; 3]) {
        for (c, plane) in self.planes.iter().enumerate() {
            for sy in 0..SH {
                let start = (row * SH + sy) * self.width + col * SW;
                out[c][sy * SW..(sy + 1) * SW].copy_from_slice(&plane[start..start + SW]);
            }
        }
    }
}

/// Three dot products (one per channel) of a mask with the samples, with
/// eight partial sums so the loop vectorizes.
#[inline(always)]
fn dot3(m: &Mask, s: &[[f32; N]; 3]) -> [f32; 3] {
    let mut acc = [[0.0f32; 8]; 3];
    for k in (0..N).step_by(8) {
        for j in 0..8 {
            let mv = m[k + j];
            acc[0][j] += mv * s[0][k + j];
            acc[1][j] += mv * s[1][k + j];
            acc[2][j] += mv * s[2][k + j];
        }
    }
    acc.map(|a| a.iter().sum())
}

/// What pass 1 keeps about a candidate.
#[derive(Clone, Copy, Debug)]
struct Cand {
    glyph: u16,
    err: f32,
    /// Σ m·S per channel.
    dm: [f32; 3],
    /// Σ m_b·S_b per channel.
    dmb: [f32; 3],
}

/// Per-cell sums of the samples, taken relative to the cell's mean color
/// `mu` so that flat cells give exactly zero error for every glyph (no f32
/// cancellation).
#[derive(Clone, Copy, Debug, Default)]
struct CellSums {
    mu: [f32; 3],
    s: [f32; 3],
    sb: [f32; 3],
    sb2: [f32; 3],
}

impl CellSums {
    /// The sums after every sample moves by `e`.
    fn shifted(&self, e: [f32; 3]) -> CellSums {
        let n = N as f32;
        CellSums {
            mu: self.mu,
            s: [0, 1, 2].map(|c| self.s[c] + n * e[c]),
            sb: [0, 1, 2].map(|c| self.sb[c] + n * e[c]),
            sb2: [0, 1, 2].map(|c| self.sb2[c] + 2.0 * e[c] * self.sb[c] + n * e[c] * e[c]),
        }
    }
}

const MAX_K: usize = 6;
/// Error difference below which two candidates count as equally good.
const TIE_MARGIN: f32 = 1e-6;

/// Mean colors of covered and uncovered samples for a candidate (relative
/// to the cell mean, like the sums).
fn means(set: &GlyphSet, g: usize, dm: [f32; 3], s: [f32; 3]) -> (Lab, Lab) {
    let n = N as f32;
    let sm = set.sum[g];
    let mut fg = [0.0; 3];
    let mut bg = [0.0; 3];
    for c in 0..3 {
        let all = s[c] / n;
        fg[c] = if sm > 1e-3 { dm[c] / sm } else { all };
        bg[c] = if n - sm > 1e-3 { (s[c] - dm[c]) / (n - sm) } else { all };
    }
    (fg, bg)
}

fn add(a: Lab, b: Lab) -> Lab {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: Lab, b: Lab) -> Lab {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Error of drawing candidate `g` with colors `fg`/`bg`, against the blurred
/// samples (sums in `sums`, cross terms in `dmb`; all relative to the mean).
fn error(set: &GlyphSet, g: usize, fg: Lab, bg: Lab, dmb: [f32; 3], sums: &CellSums) -> f32 {
    let n = N as f32;
    let mut e = 0.0;
    for c in 0..3 {
        let u = fg[c] - bg[c];
        e += sums.sb2[c] - 2.0 * (bg[c] * sums.sb[c] + u * dmb[c])
            + n * bg[c] * bg[c]
            + 2.0 * bg[c] * u * set.sum_b[g]
            + u * u * set.sum_b2[g];
    }
    e
}

/// Pass 1 for one cell: evaluate every glyph, keep the best `k`. Centers
/// `s` and `sb` on the cell mean in place.
fn evaluate_cell(
    set: &GlyphSet,
    s: &mut [[f32; N]; 3],
    sb: &mut [[f32; N]; 3],
    blur: bool,
    k: usize,
) -> (CellSums, [Cand; MAX_K], usize) {
    let mut sums = CellSums::default();
    for c in 0..3 {
        let mu = s[c].iter().sum::<f32>() / N as f32;
        sums.mu[c] = mu;
        s[c].iter_mut().for_each(|v| *v -= mu);
        sb[c].iter_mut().for_each(|v| *v -= mu);
        sums.s[c] = s[c].iter().sum();
        sums.sb[c] = sb[c].iter().sum();
        sums.sb2[c] = sb[c].iter().map(|v| v * v).sum();
    }
    let mut best = [Cand {
        glyph: 0,
        err: f32::MAX,
        dm: [0.0; 3],
        dmb: [0.0; 3],
    }; MAX_K];
    // A flat cell (most of a page once text is left out) is a blank glyph;
    // no need to try every mask.
    let flat = |a: &[[f32; N]; 3]| a.iter().all(|ch| ch.iter().all(|v| v.abs() < 1e-4));
    if flat(s) && flat(sb) {
        best[0] = Cand {
            glyph: set.blank_index() as u16,
            err: 0.0,
            dm: [0.0; 3],
            dmb: [0.0; 3],
        };
        return (sums, best, 1);
    }
    let mut count = 0;
    for g in 0..set.len() {
        let dm = dot3(&set.glyphs[g].mask, s);
        let dmb = if blur { dot3(&set.blurred[g], sb) } else { dm };
        let (fg, bg) = means(set, g, dm, sums.s);
        let err = if blur {
            error(set, g, fg, bg, dmb, &sums)
        } else {
            // Without blur: total variance minus what the two means explain.
            let n = N as f32;
            let sm = set.sum[g];
            let mut e = 0.0;
            for c in 0..3 {
                e += sums.sb2[c] - sm * fg[c] * fg[c] - (n - sm) * bg[c] * bg[c];
            }
            e
        };
        // Insert into the sorted top-k list. A later glyph must be better by
        // a margin, so ties (flat cells) keep the earlier, simpler glyph.
        if count < k || err < best[count - 1].err - TIE_MARGIN {
            let mut i = count.min(k - 1);
            while i > 0 && best[i - 1].err > err {
                best[i] = best[i - 1];
                i -= 1;
            }
            best[i] = Cand {
                glyph: g as u16,
                err,
                dm,
                dmb,
            };
            count = (count + 1).min(k);
        }
    }
    (sums, best, count)
}

/// Choose glyphs and colors for a `cols` x `rows` grid of cells from an image
/// of exactly `cols * SW` x `rows * SH` RGBA pixels.
pub fn optimize(rgba: &[u8], cols: usize, rows: usize, set: &GlyphSet, opts: &Options) -> Vec<RasterCell> {
    let img = LabImage::from_rgba(rgba, cols * SW, rows * SH);
    optimize_lab(&img, cols, rows, set, opts)
}

/// [`optimize`] on an image already converted to OKLab.
pub fn optimize_lab(img: &LabImage, cols: usize, rows: usize, set: &GlyphSet, opts: &Options) -> Vec<RasterCell> {
    assert_eq!((img.width, img.height), (cols * SW, rows * SH));
    let palette = Palette::for_depth(opts.depth);
    let blurred;
    let src_b = if opts.blur {
        blurred = img.blurred();
        &blurred
    } else {
        img
    };
    let k = if palette.is_some() { MAX_K } else { 1 };

    // Pass 1, in parallel over rows of cells.
    let pass1: Vec<(CellSums, [Cand; MAX_K], usize)> = (0..rows)
        .into_par_iter()
        .flat_map_iter(|row| {
            let mut s = [[0.0f32; N]; 3];
            let mut sb = [[0.0f32; N]; 3];
            (0..cols)
                .map(|col| {
                    img.gather(col, row, &mut s);
                    if opts.blur {
                        src_b.gather(col, row, &mut sb);
                    } else {
                        sb = s;
                    }
                    evaluate_cell(set, &mut s, &mut sb, opts.blur, k)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let Some(palette) = palette else {
        let mut cells: Vec<RasterCell> = pass1
            .iter()
            .map(|(sums, best, _)| {
                let c = best[0];
                let (fg, bg) = means(set, c.glyph as usize, c.dm, sums.s);
                RasterCell {
                    glyph: c.glyph,
                    fg: add(fg, sums.mu),
                    bg: add(bg, sums.mu),
                }
            })
            .collect();
        if opts.coherence > 0.0 {
            cohere(&mut cells, cols, opts.coherence);
        }
        return cells;
    };

    // Pass 2: quantize, with error diffusion, row by row.
    let q = Quantizer {
        palette,
        extras: opts.extras,
    };
    let n = N as f32;
    let mut out = Vec::with_capacity(cols * rows);
    let mut err_cur = vec![[0.0f32; 3]; cols + 2];
    let mut err_next = vec![[0.0f32; 3]; cols + 2];
    for row in 0..rows {
        for col in 0..cols {
            let (sums, best, count) = &pass1[row * cols + col];
            let e = if opts.diffusion { err_cur[col + 1] } else { [0.0; 3] };
            let shifted = sums.shifted(e);
            let mut choice: Option<(f32, u16, Lab, Lab)> = None;
            for cand in &best[..*count] {
                let g = cand.glyph as usize;
                let dm = [0, 1, 2].map(|c| cand.dm[c] + e[c] * set.sum[g]);
                let dmb = [0, 1, 2].map(|c| cand.dmb[c] + e[c] * set.sum_b[g]);
                let (fg, bg) = means(set, g, dm, shifted.s);
                let fq = q.quantize(add(fg, sums.mu));
                let bq = q.quantize(add(bg, sums.mu));
                let err = error(set, g, sub(fq, sums.mu), sub(bq, sums.mu), dmb, &shifted);
                if choice.is_none_or(|c| err < c.0 - TIE_MARGIN) {
                    choice = Some((err, cand.glyph, fq, bq));
                }
            }
            let (_, glyph, fq, bq) = choice.expect("at least one candidate");
            out.push(RasterCell { glyph, fg: fq, bg: bq });
            if opts.diffusion {
                // Mean color error of the cell, spread to the neighbours.
                let cov = set.sum[glyph as usize] / n;
                for c in 0..3 {
                    let mean_src = shifted.s[c] / n + sums.mu[c];
                    let mean_out = bq[c] + (fq[c] - bq[c]) * cov;
                    let d = (mean_src - mean_out).clamp(-0.25, 0.25);
                    err_cur[col + 2][c] += d * 7.0 / 16.0;
                    err_next[col][c] += d * 3.0 / 16.0;
                    err_next[col + 1][c] += d * 5.0 / 16.0;
                    err_next[col + 2][c] += d / 16.0;
                }
            }
        }
        std::mem::swap(&mut err_cur, &mut err_next);
        err_next.iter_mut().for_each(|e| *e = [0.0; 3]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::rgb_to_lab;
    use sshpdf_term::Tier;

    /// An image of one cell per mask pattern: black where the mask is set.
    fn image_of(masks: &[Mask], colors: ([u8; 3], [u8; 3])) -> Vec<u8> {
        let cols = masks.len();
        let mut img = vec![0u8; cols * SW * SH * 4];
        for (c, m) in masks.iter().enumerate() {
            for sy in 0..SH {
                for sx in 0..SW {
                    let v = m[sy * SW + sx];
                    let px = if v > 0.5 { colors.0 } else { colors.1 };
                    let i = (sy * cols * SW + c * SW + sx) * 4;
                    img[i..i + 3].copy_from_slice(&px);
                    img[i + 3] = 255;
                }
            }
        }
        img
    }

    #[test]
    fn reproduces_every_octant_pattern_exactly() {
        let set = GlyphSet::get(Tier::Octant, false);
        let masks: Vec<Mask> = set.glyphs.iter().map(|g| g.mask).collect();
        let img = image_of(&masks, ([200, 30, 30], [250, 250, 240]));
        let cells = optimize(&img, masks.len(), 1, set, &Options::default());
        for (i, c) in cells.iter().enumerate() {
            let g = c.glyph as usize;
            let same = g == i || set.complement(i) == Some(g);
            assert!(same, "pattern {i} drew glyph {g}");
        }
    }

    #[test]
    fn flat_color_gives_that_color() {
        let set = GlyphSet::get(Tier::Half, false);
        let img = image_of(&[[0.0; N]], ([0, 0, 0], [10, 120, 200]));
        let cells = optimize(&img, 1, 1, set, &Options::default());
        let lab = rgb_to_lab([10, 120, 200]);
        let c = cells[0];
        let shown = if set.coverage(c.glyph as usize) > 0.5 {
            c.fg
        } else {
            c.bg
        };
        assert!(crate::color::dist2(shown, lab) < 1e-6);
    }

    #[test]
    fn thin_line_is_kept_with_blur() {
        // A one-sample-wide vertical black line in the left half of a cell:
        // the best quadrant glyph must still put ink on the left.
        let mut m = [0.0f32; N];
        for y in 0..SH {
            m[y * SW + 2] = 1.0;
        }
        let set = GlyphSet::get(Tier::Quadrant, false);
        let img = image_of(&[m], ([0, 0, 0], [255, 255, 255]));
        let c = optimize(&img, 1, 1, set, &Options::default())[0];
        let g = &set.glyphs[c.glyph as usize];
        assert!(g.ch == '▌' || g.ch == '▐', "{}", g.ch);
        // The left part is darker than the right.
        let left = if g.ch == '▌' { c.fg } else { c.bg };
        let right = if g.ch == '▌' { c.bg } else { c.fg };
        assert!(left[0] < right[0]);
    }

    #[test]
    fn palette_output_is_quantized() {
        let set = GlyphSet::get(Tier::Half, false);
        let img = image_of(&[[0.0; N]; 4], ([0, 0, 0], [120, 77, 33]));
        let p = Palette::for_depth(ColorDepth::Ansi256).unwrap();
        let opts = Options {
            depth: ColorDepth::Ansi256,
            ..Options::default()
        };
        for c in optimize(&img, 4, 1, set, &opts) {
            assert!(p.lab.contains(&c.fg));
            assert!(p.lab.contains(&c.bg));
        }
    }

    #[test]
    fn diffusion_dithers_flat_areas() {
        // A flat color between two palette entries should alternate between
        // neighbouring colors rather than show one constant color.
        let set = GlyphSet::get(Tier::Half, false);
        let cols = 16;
        // Gray 64 lies between black and the palette's gray 127.
        let img = image_of(&vec![[0.0; N]; cols], ([0, 0, 0], [64, 64, 64]));
        let opts = Options {
            depth: ColorDepth::Ansi16,
            ..Options::default()
        };
        let cells = optimize(&img, cols, 1, set, &opts);
        let shown: std::collections::HashSet<[u32; 3]> = cells
            .iter()
            .map(|c| {
                let v = if set.coverage(c.glyph as usize) > 0.5 {
                    c.fg
                } else {
                    c.bg
                };
                v.map(|x| (x * 1000.0) as u32)
            })
            .collect();
        assert!(shown.len() >= 2, "{shown:?}");
    }
}
