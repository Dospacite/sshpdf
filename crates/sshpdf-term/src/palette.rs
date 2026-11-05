//! The xterm color palettes.
//!
//! The real 16-color palette of a terminal is unknown (themes change it), so
//! the xterm defaults stand in for it.

/// The xterm default colors for indices 0-15.
pub const ANSI16: [[u8; 3]; 16] = [
    [0, 0, 0],
    [205, 0, 0],
    [0, 205, 0],
    [205, 205, 0],
    [0, 0, 238],
    [205, 0, 205],
    [0, 205, 205],
    [229, 229, 229],
    [127, 127, 127],
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [92, 92, 255],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
];

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// RGB value of xterm palette entry `i` (0-255).
pub fn xterm_rgb(i: u8) -> [u8; 3] {
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            let n = i - 16;
            [
                CUBE_LEVELS[(n / 36) as usize],
                CUBE_LEVELS[((n / 6) % 6) as usize],
                CUBE_LEVELS[(n % 6) as usize],
            ]
        }
        232..=255 => {
            let v = 8 + 10 * (i - 232);
            [v, v, v]
        }
    }
}

fn dist2(a: [u8; 3], b: [u8; 3]) -> u32 {
    // Weighted RGB distance; good enough as a fallback. The raster layer
    // does its own quantization in OKLab.
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    (2 * dr * dr + 4 * dg * dg + 3 * db * db) as u32
}

/// Nearest palette index for an RGB color: among entries 16-255 for the
/// 256-color palette (its values are fixed), or 0-15 for the 16-color one.
pub fn nearest(rgb: [u8; 3], colors: u16) -> u8 {
    let range = if colors <= 16 { 0..=15u8 } else { 16..=255u8 };
    range
        .min_by_key(|&i| dist2(rgb, xterm_rgb(i)))
        .expect("non-empty palette")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_and_grays() {
        assert_eq!(xterm_rgb(16), [0, 0, 0]);
        assert_eq!(xterm_rgb(231), [255, 255, 255]);
        assert_eq!(xterm_rgb(196), [255, 0, 0]);
        assert_eq!(xterm_rgb(232), [8, 8, 8]);
        assert_eq!(xterm_rgb(255), [238, 238, 238]);
    }

    #[test]
    fn nearest_finds_exact_entries() {
        for i in 16..=255u8 {
            assert_eq!(xterm_rgb(nearest(xterm_rgb(i), 256)), xterm_rgb(i));
        }
        assert_eq!(nearest([250, 10, 10], 16), 9);
    }
}
