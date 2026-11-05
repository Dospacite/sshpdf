//! Box-drawing characters by the weight of their four arms.

include!(concat!(env!("OUT_DIR"), "/box_chars.rs"));

/// Arm weight.
pub const NONE: u8 = 0;
pub const LIGHT: u8 = 1;
pub const HEAVY: u8 = 2;
pub const DOUBLE: u8 = 3;

/// Arm indices.
pub const UP: usize = 0;
pub const DOWN: usize = 1;
pub const LEFT: usize = 2;
pub const RIGHT: usize = 3;

fn exact(arms: [u8; 4]) -> Option<char> {
    BOX_CHARS.iter().find(|(a, _)| *a == arms).map(|(_, c)| *c)
}

/// The box-drawing character for arms `[up, down, left, right]`.
///
/// Combinations Unicode doesn't have (double mixed with heavy, double half
/// lines) fall back to the closest one: heavy arms next to double ones become
/// light, then double arms become light, keeping the shape.
pub fn box_char(arms: [u8; 4]) -> Option<char> {
    if arms == [NONE; 4] {
        return None;
    }
    if let Some(c) = exact(arms) {
        return Some(c);
    }
    let map = |f: &dyn Fn(u8) -> u8| arms.map(f);
    let tries = [
        map(&|w| if w == HEAVY { LIGHT } else { w }),
        map(&|w| if w == DOUBLE { LIGHT } else { w }),
        map(&|w| if w == NONE { NONE } else { LIGHT }),
    ];
    // Every light-only combination exists, so the last try always succeeds.
    tries.into_iter().find_map(exact)
}

/// The arms of a box-drawing character (inverse of [`box_char`]).
pub fn arms_of(ch: char) -> Option<[u8; 4]> {
    BOX_CHARS.iter().find(|(_, c)| *c == ch).map(|(a, _)| *a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_junctions() {
        assert_eq!(box_char([NONE, NONE, LIGHT, LIGHT]), Some('─'));
        assert_eq!(box_char([LIGHT, LIGHT, NONE, NONE]), Some('│'));
        assert_eq!(box_char([NONE, LIGHT, NONE, LIGHT]), Some('┌'));
        assert_eq!(box_char([NONE, LIGHT, LIGHT, NONE]), Some('┐'));
        assert_eq!(box_char([LIGHT, NONE, NONE, LIGHT]), Some('└'));
        assert_eq!(box_char([LIGHT, NONE, LIGHT, NONE]), Some('┘'));
        assert_eq!(box_char([LIGHT, LIGHT, NONE, LIGHT]), Some('├'));
        assert_eq!(box_char([LIGHT, LIGHT, LIGHT, NONE]), Some('┤'));
        assert_eq!(box_char([NONE, LIGHT, LIGHT, LIGHT]), Some('┬'));
        assert_eq!(box_char([LIGHT, NONE, LIGHT, LIGHT]), Some('┴'));
        assert_eq!(box_char([LIGHT; 4]), Some('┼'));
        assert_eq!(box_char([NONE; 4]), None);
    }

    #[test]
    fn heavy_and_double() {
        assert_eq!(box_char([NONE, NONE, HEAVY, HEAVY]), Some('━'));
        assert_eq!(box_char([HEAVY, HEAVY, NONE, NONE]), Some('┃'));
        assert_eq!(box_char([NONE, HEAVY, NONE, HEAVY]), Some('┏'));
        assert_eq!(box_char([LIGHT, LIGHT, HEAVY, HEAVY]), Some('┿'));
        assert_eq!(box_char([NONE, NONE, DOUBLE, DOUBLE]), Some('═'));
        assert_eq!(box_char([DOUBLE, DOUBLE, DOUBLE, DOUBLE]), Some('╬'));
        assert_eq!(box_char([LIGHT, LIGHT, DOUBLE, DOUBLE]), Some('╪'));
    }

    #[test]
    fn every_combination_gives_a_char() {
        for u in 0..4u8 {
            for d in 0..4u8 {
                for l in 0..4u8 {
                    for r in 0..4u8 {
                        let arms = [u, d, l, r];
                        if arms != [NONE; 4] {
                            assert!(box_char(arms).is_some(), "{arms:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fallbacks_keep_the_shape() {
        // Double mixed with heavy: heavy becomes light.
        assert_eq!(box_char([HEAVY, HEAVY, DOUBLE, DOUBLE]), Some('╪'));
        // A lone double half line keeps its shape, drawn light.
        assert_eq!(box_char([NONE, NONE, DOUBLE, NONE]), Some('╴'));
    }
}
