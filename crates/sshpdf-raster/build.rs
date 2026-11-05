//! Generates the glyph tables from `data/UnicodeData.txt`:
//!
//! - octants: all 256 patterns of the 2x4 sub-cell grid,
//! - sextants: all 64 patterns of the 2x3 grid,
//! - wedges: the Legacy Computing "BLOCK DIAGONAL" characters as half-planes.
//!
//! Sub-cell numbering follows the Unicode names (`BLOCK OCTANT-1235`): row
//! by row, left to right, so octant positions are
//!
//! ```text
//! 1 2
//! 3 4
//! 5 6
//! 7 8
//! ```
//!
//! Bit `n - 1` of a pattern stands for position `n`. The patterns not named
//! in the octant/sextant ranges are covered by older characters; the build
//! fails unless every pattern is covered exactly once.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Older characters that cover octant patterns, with their Unicode names
/// (checked against UnicodeData.txt) and the octant positions they fill.
const OCTANT_EXTRAS: &[(u32, &str, &str)] = &[
    (0x0020, "SPACE", ""),
    (0x2588, "FULL BLOCK", "12345678"),
    (0x2580, "UPPER HALF BLOCK", "1234"),
    (0x2584, "LOWER HALF BLOCK", "5678"),
    (0x258C, "LEFT HALF BLOCK", "1357"),
    (0x2590, "RIGHT HALF BLOCK", "2468"),
    (0x2598, "QUADRANT UPPER LEFT", "13"),
    (0x259D, "QUADRANT UPPER RIGHT", "24"),
    (0x2596, "QUADRANT LOWER LEFT", "57"),
    (0x2597, "QUADRANT LOWER RIGHT", "68"),
    (0x259A, "QUADRANT UPPER LEFT AND LOWER RIGHT", "1368"),
    (0x259E, "QUADRANT UPPER RIGHT AND LOWER LEFT", "2457"),
    (0x259B, "QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT", "123457"),
    (0x259C, "QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT", "123468"),
    (0x2599, "QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT", "135678"),
    (0x259F, "QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT", "245678"),
    (0x2582, "LOWER ONE QUARTER BLOCK", "78"),
    (0x2586, "LOWER THREE QUARTERS BLOCK", "345678"),
    (0x1FB82, "UPPER ONE QUARTER BLOCK", "12"),
    (0x1FB85, "UPPER THREE QUARTERS BLOCK", "123456"),
    (0x1CEA8, "LEFT HALF UPPER ONE QUARTER BLOCK", "1"),
    (0x1CEAB, "RIGHT HALF UPPER ONE QUARTER BLOCK", "2"),
    (0x1CEA3, "LEFT HALF LOWER ONE QUARTER BLOCK", "7"),
    (0x1CEA0, "RIGHT HALF LOWER ONE QUARTER BLOCK", "8"),
    (0x1FBE6, "MIDDLE LEFT ONE QUARTER BLOCK", "35"),
    (0x1FBE7, "MIDDLE RIGHT ONE QUARTER BLOCK", "46"),
];

/// Older characters that cover sextant patterns (positions 1-6, row by row).
const SEXTANT_EXTRAS: &[(u32, &str, &str)] = &[
    (0x0020, "SPACE", ""),
    (0x2588, "FULL BLOCK", "123456"),
    (0x258C, "LEFT HALF BLOCK", "135"),
    (0x2590, "RIGHT HALF BLOCK", "246"),
];

fn pattern(digits: &str) -> u32 {
    digits
        .chars()
        .map(|d| 1u32 << (d.to_digit(10).expect("digit") - 1))
        .fold(0, |a, b| a | b)
}

/// A point on the cell outline named in a "BLOCK DIAGONAL" name, in cell
/// units (x right, y down, both 0..1).
fn named_point(name: &str) -> Option<(f32, f32)> {
    Some(match name {
        "UPPER LEFT" => (0.0, 0.0),
        "UPPER CENTRE" => (0.5, 0.0),
        "UPPER RIGHT" => (1.0, 0.0),
        "LOWER LEFT" => (0.0, 1.0),
        "LOWER CENTRE" => (0.5, 1.0),
        "LOWER RIGHT" => (1.0, 1.0),
        "UPPER MIDDLE LEFT" => (0.0, 1.0 / 3.0),
        "LOWER MIDDLE LEFT" => (0.0, 2.0 / 3.0),
        "UPPER MIDDLE RIGHT" => (1.0, 1.0 / 3.0),
        "LOWER MIDDLE RIGHT" => (1.0, 2.0 / 3.0),
        _ => return None,
    })
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let data = manifest.join("../../data/UnicodeData.txt");
    println!("cargo:rerun-if-changed={}", data.display());
    let text = std::fs::read_to_string(&data).unwrap_or_else(|e| panic!("cannot read {}: {e}", data.display()));

    let mut names: HashMap<u32, String> = HashMap::new();
    for line in text.lines() {
        let mut f = line.split(';');
        let cp = u32::from_str_radix(f.next().expect("code point"), 16).expect("hex");
        let name = f.next().expect("name").to_string();
        names.insert(cp, name);
    }
    // Space has the name "SPACE" in UnicodeData; check every extra by name.
    for (cp, name, _) in OCTANT_EXTRAS.iter().chain(SEXTANT_EXTRAS) {
        let actual = names.get(cp).map(String::as_str).unwrap_or("<missing>");
        assert_eq!(actual, *name, "U+{cp:04X} has an unexpected name");
    }

    let mut octants: Vec<Option<u32>> = vec![None; 256];
    let mut sextants: Vec<Option<u32>> = vec![None; 64];
    let mut wedges = String::new();
    let mut cps: Vec<&u32> = names.keys().collect();
    cps.sort();
    for cp in cps {
        let name = &names[cp];
        if let Some(d) = name.strip_prefix("BLOCK OCTANT-") {
            let p = pattern(d) as usize;
            assert!(octants[p].is_none(), "octant pattern {p:08b} twice");
            octants[p] = Some(*cp);
        } else if let Some(d) = name.strip_prefix("BLOCK SEXTANT-") {
            let p = pattern(d) as usize;
            assert!(sextants[p].is_none(), "sextant pattern {p:06b} twice");
            sextants[p] = Some(*cp);
        } else if (0x1FB3C..=0x1FB67).contains(cp) {
            // "<UPPER|LOWER> <LEFT|RIGHT> BLOCK DIAGONAL <A> TO <B>": the
            // part of the cell on the named corner's side of the line A-B.
            let (corner, line) = name
                .split_once(" BLOCK DIAGONAL ")
                .unwrap_or_else(|| panic!("unexpected wedge name {name}"));
            let (a, b) = line.split_once(" TO ").expect("A TO B");
            let c = named_point(corner).unwrap_or_else(|| panic!("corner in {name}"));
            let a = named_point(a).unwrap_or_else(|| panic!("point in {name}"));
            let b = named_point(b).unwrap_or_else(|| panic!("point in {name}"));
            writeln!(
                wedges,
                "    ('\\u{{{cp:X}}}', [{:?}, {:?}], [{:?}, {:?}], [{:?}, {:?}]),",
                a.0, a.1, b.0, b.1, c.0, c.1
            )
            .expect("write");
        }
    }
    for (cp, _, d) in OCTANT_EXTRAS {
        let p = pattern(d) as usize;
        assert!(octants[p].is_none(), "octant pattern {p:08b} twice (U+{cp:04X})");
        octants[p] = Some(*cp);
    }
    for (cp, _, d) in SEXTANT_EXTRAS {
        let p = pattern(d) as usize;
        assert!(sextants[p].is_none(), "sextant pattern {p:06b} twice (U+{cp:04X})");
        sextants[p] = Some(*cp);
    }

    let mut out = String::new();
    out.push_str("// Generated by build.rs from data/UnicodeData.txt. Do not edit.\n\n");
    out.push_str("/// Character for each octant pattern (index = pattern bits).\n");
    out.push_str("pub(crate) const OCTANTS: [char; 256] = [\n");
    for (p, cp) in octants.iter().enumerate() {
        let cp = cp.unwrap_or_else(|| panic!("octant pattern {p:08b} not covered"));
        writeln!(out, "    '\\u{{{cp:X}}}',").expect("write");
    }
    out.push_str("];\n\n/// Character for each sextant pattern (index = pattern bits).\n");
    out.push_str("pub(crate) const SEXTANTS: [char; 64] = [\n");
    for (p, cp) in sextants.iter().enumerate() {
        let cp = cp.unwrap_or_else(|| panic!("sextant pattern {p:06b} not covered"));
        writeln!(out, "    '\\u{{{cp:X}}}',").expect("write");
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// Wedges: (char, line point A, line point B, corner on the filled side),\n\
         /// in cell units (x right, y down).\n",
    );
    out.push_str("#[allow(clippy::type_complexity)]\n");
    out.push_str("pub(crate) const WEDGES: &[(char, [f32; 2], [f32; 2], [f32; 2])] = &[\n");
    out.push_str(&wedges);
    out.push_str("];\n");

    let out_path = PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("glyph_tables.rs");
    std::fs::write(out_path, out).expect("write glyph tables");
}
