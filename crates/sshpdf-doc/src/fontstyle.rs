//! Bold / italic / monospace from font names and font descriptor flags.

/// Remove a subset prefix like `ABCDEF+`.
pub fn strip_subset(name: &str) -> &str {
    match name.split_once('+') {
        Some((prefix, rest)) if prefix.len() == 6 && prefix.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// Style guessed from a PostScript font name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NameStyle {
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
}

/// TeX font name prefixes (Computer Modern, Latin Modern, AMS) and their style.
const TEX_PREFIXES: &[(&str, NameStyle)] = &[
    (
        "cmbxti",
        NameStyle {
            bold: true,
            italic: true,
            mono: false,
        },
    ),
    (
        "cmbxsl",
        NameStyle {
            bold: true,
            italic: true,
            mono: false,
        },
    ),
    (
        "cmbx",
        NameStyle {
            bold: true,
            italic: false,
            mono: false,
        },
    ),
    (
        "cmssbx",
        NameStyle {
            bold: true,
            italic: false,
            mono: false,
        },
    ),
    (
        "cmb",
        NameStyle {
            bold: true,
            italic: false,
            mono: false,
        },
    ),
    (
        "cmmib",
        NameStyle {
            bold: true,
            italic: true,
            mono: false,
        },
    ),
    (
        "cmbsy",
        NameStyle {
            bold: true,
            italic: false,
            mono: false,
        },
    ),
    (
        "cmti",
        NameStyle {
            bold: false,
            italic: true,
            mono: false,
        },
    ),
    (
        "cmsl",
        NameStyle {
            bold: false,
            italic: true,
            mono: false,
        },
    ),
    (
        "cmssi",
        NameStyle {
            bold: false,
            italic: true,
            mono: false,
        },
    ),
    (
        "cmmi",
        NameStyle {
            bold: false,
            italic: true,
            mono: false,
        },
    ),
    (
        "cmitt",
        NameStyle {
            bold: false,
            italic: true,
            mono: true,
        },
    ),
    (
        "cmsltt",
        NameStyle {
            bold: false,
            italic: true,
            mono: true,
        },
    ),
    (
        "cmtt",
        NameStyle {
            bold: false,
            italic: false,
            mono: true,
        },
    ),
    (
        "cmvtt",
        NameStyle {
            bold: false,
            italic: false,
            mono: true,
        },
    ),
];

pub fn style_from_name(name: &str) -> NameStyle {
    let n = strip_subset(name).to_ascii_lowercase();
    for (prefix, style) in TEX_PREFIXES {
        // Only when followed by the design size (cmbx10, cmti12).
        if let Some(rest) = n.strip_prefix(prefix)
            && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            return *style;
        }
    }
    // The style part, usually after '-' or ','; whole name otherwise.
    let style_part = n.rsplit_once(['-', ',']).map(|(_, s)| s).unwrap_or(&n);
    let any = |words: &[&str]| words.iter().any(|w| n.contains(w));
    let bold = any(&[
        "bold",
        "black",
        "heavy",
        "semibold",
        "demibold",
        "extrabold",
        "ultrabold",
    ]) || style_part.starts_with("demi")
        || style_part == "medi"
        || style_part.starts_with("medi") && style_part.contains("ital")
        || style_part == "bd"
        || style_part == "b"
        || style_part == "bi"
        || style_part == "sb";
    let italic = any(&["italic", "oblique", "ital", "inclined", "slanted"])
        || style_part == "it"
        || style_part == "i"
        || style_part == "bi"
        || style_part.ends_with("it") && style_part.len() <= 6;
    let mono = any(&["mono", "courier", "consol", "code", "typewriter", "fixed"])
        || n.starts_with("lmmono")
        || n.starts_with("sfmono")
        || n.starts_with("menlo");
    NameStyle { bold, italic, mono }
}

/// PDF font descriptor flags (PDF 32000 table 121).
pub mod flags {
    pub const FIXED_PITCH: u32 = 1;
    pub const ITALIC: u32 = 1 << 6;
    pub const FORCE_BOLD: u32 = 1 << 18;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str) -> (bool, bool, bool) {
        let st = style_from_name(name);
        (st.bold, st.italic, st.mono)
    }

    #[test]
    fn common_font_names() {
        assert_eq!(s("ABCDEF+Times-Bold"), (true, false, false));
        assert_eq!(s("Times-BoldItalic"), (true, true, false));
        assert_eq!(s("Helvetica-Oblique"), (false, true, false));
        assert_eq!(s("Courier"), (false, false, true));
        assert_eq!(s("NimbusRomNo9L-Medi"), (true, false, false));
        assert_eq!(s("NimbusRomNo9L-ReguItal"), (false, true, false));
        assert_eq!(s("NimbusRomNo9L-MediItal"), (true, true, false));
        assert_eq!(s("ArialMT"), (false, false, false));
        assert_eq!(s("Arial-BoldMT"), (true, false, false));
        assert_eq!(s("LinLibertineO"), (false, false, false));
        assert_eq!(s("SourceSansPro-Semibold"), (true, false, false));
        assert_eq!(s("DejaVuSansMono"), (false, false, true));
        assert_eq!(s("Inter-Medium"), (false, false, false));
    }

    #[test]
    fn tex_font_names() {
        assert_eq!(s("XYZABC+CMBX10"), (true, false, false));
        assert_eq!(s("CMTI10"), (false, true, false));
        assert_eq!(s("CMMI7"), (false, true, false));
        assert_eq!(s("CMR10"), (false, false, false));
        assert_eq!(s("CMTT9"), (false, false, true));
        assert_eq!(s("LMRoman10-Bold"), (true, false, false));
        assert_eq!(s("LMRoman10-Regular"), (false, false, false));
    }

    #[test]
    fn subset_prefix() {
        assert_eq!(strip_subset("ABCDEF+Foo"), "Foo");
        assert_eq!(strip_subset("Foo+Bar"), "Foo+Bar");
    }
}
