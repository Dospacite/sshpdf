//! Terminal capabilities, detected from environment variables only.
//!
//! Inside tmux, escape-sequence queries are answered by tmux rather than the
//! outer terminal, so sshpdf never queries; it guesses conservatively from the
//! environment and lets the user override everything (CLI flags, config file,
//! `--probe --save`).

use std::fmt;

/// A glyph set for the raster layer, from coarsest to finest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// ` .:-=+*#%@` density ramp; works everywhere.
    Ascii,
    /// ` ▀▄█`: 1×2 sub-cells; works everywhere including conhost.
    Half,
    /// Half blocks plus quadrants: 2×2 sub-cells.
    Quadrant,
    /// Unicode 13 sextants: 2×3 sub-cells.
    Sextant,
    /// Unicode 16 octants: 2×4 sub-cells.
    Octant,
    /// Braille dots, 2×4: good for line art only.
    Braille,
}

impl Tier {
    pub const ALL: [Tier; 6] = [
        Tier::Ascii,
        Tier::Half,
        Tier::Quadrant,
        Tier::Sextant,
        Tier::Octant,
        Tier::Braille,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tier::Ascii => "ascii",
            Tier::Half => "half",
            Tier::Quadrant => "quadrant",
            Tier::Sextant => "sextant",
            Tier::Octant => "octant",
            Tier::Braille => "braille",
        }
    }

    pub fn parse(s: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.name() == s)
    }

    /// Whether the tier uses characters that are new enough for terminals
    /// (and tmux) to disagree about their width.
    pub fn needs_robust_output(self) -> bool {
        matches!(self, Tier::Sextant | Tier::Octant)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How many colors the terminal can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColorDepth {
    Ansi16,
    Ansi256,
    True,
}

impl ColorDepth {
    pub fn name(self) -> &'static str {
        match self {
            ColorDepth::Ansi16 => "16",
            ColorDepth::Ansi256 => "256",
            ColorDepth::True => "true",
        }
    }

    pub fn parse(s: &str) -> Option<ColorDepth> {
        match s {
            "16" => Some(ColorDepth::Ansi16),
            "256" => Some(ColorDepth::Ansi256),
            "true" | "truecolor" | "24bit" => Some(ColorDepth::True),
            _ => None,
        }
    }
}

impl fmt::Display for ColorDepth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What sshpdf assumes about the terminal it draws on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caps {
    /// The detected terminal, for the info screen ("kitty", "tmux", ...).
    pub terminal: String,
    pub tier: Tier,
    /// Legacy Computing shapes (wedges, diagonals) and shade/eighth blocks
    /// as extra raster glyphs. Only used with tiers ≥ sextant.
    pub shapes: bool,
    pub depth: ColorDepth,
    /// Reposition the cursor after characters of uncertain width.
    pub robust: bool,
    /// Colored underlines (SGR 58) as a third color in a cell.
    pub underline_color: bool,
    /// Running inside tmux.
    pub tmux: bool,
    /// Native Windows console host without Windows Terminal.
    pub conhost: bool,
    /// Whether the terminal background is dark, if known (from `COLORFGBG`).
    pub dark_background: Option<bool>,
    /// Hint to show when truecolor is likely available but not enabled.
    pub hint: Option<String>,
}

struct Profile {
    name: &'static str,
    tier: Tier,
    depth: ColorDepth,
    underline_color: bool,
}

/// Defaults for terminals we can recognize. Conservative: a tier is listed
/// only when the terminal draws those glyphs itself (built-in box/block
/// rendering) instead of depending on the font.
fn profile_for(env: &dyn Fn(&str) -> Option<String>) -> Option<Profile> {
    let term = env("TERM").unwrap_or_default();
    let program = env("TERM_PROGRAM").unwrap_or_default();
    let p = |name, tier, depth, underline_color| {
        Some(Profile {
            name,
            tier,
            depth,
            underline_color,
        })
    };
    if term == "xterm-kitty" || env("KITTY_WINDOW_ID").is_some() {
        return p("kitty", Tier::Octant, ColorDepth::True, true);
    }
    if term.starts_with("foot") {
        return p("foot", Tier::Octant, ColorDepth::True, true);
    }
    if program.eq_ignore_ascii_case("ghostty") || term == "xterm-ghostty" {
        return p("ghostty", Tier::Octant, ColorDepth::True, true);
    }
    if program == "WezTerm" || term == "wezterm" {
        return p("wezterm", Tier::Sextant, ColorDepth::True, true);
    }
    if env("WT_SESSION").is_some() {
        return p("windows-terminal", Tier::Quadrant, ColorDepth::True, false);
    }
    if term == "alacritty" || env("ALACRITTY_WINDOW_ID").is_some() {
        return p("alacritty", Tier::Quadrant, ColorDepth::True, false);
    }
    if program == "iTerm.app" {
        return p("iterm2", Tier::Quadrant, ColorDepth::True, true);
    }
    if program == "Apple_Terminal" {
        return p("apple-terminal", Tier::Quadrant, ColorDepth::Ansi256, false);
    }
    if program == "vscode" {
        return p("vscode", Tier::Quadrant, ColorDepth::True, true);
    }
    if env("KONSOLE_VERSION").is_some() {
        return p("konsole", Tier::Quadrant, ColorDepth::True, false);
    }
    if let Some(v) = env("VTE_VERSION") {
        // VTE draws block elements and Legacy Computing sextants itself.
        let depth = if v.parse::<u32>().unwrap_or(0) >= 3600 {
            ColorDepth::True
        } else {
            ColorDepth::Ansi256
        };
        return p("vte", Tier::Sextant, depth, true);
    }
    if env("XTERM_VERSION").is_some() {
        return p("xterm", Tier::Quadrant, ColorDepth::Ansi256, false);
    }
    if term == "linux" {
        return p("linux-console", Tier::Half, ColorDepth::Ansi16, false);
    }
    None
}

/// Detect capabilities from the process environment.
pub fn detect() -> Caps {
    detect_from(&|k| std::env::var(k).ok(), cfg!(windows))
}

/// Detect capabilities from the given environment lookup (testable).
pub fn detect_from(env: &dyn Fn(&str) -> Option<String>, windows: bool) -> Caps {
    let term = env("TERM").unwrap_or_default();
    let colorterm = env("COLORTERM").unwrap_or_default().to_ascii_lowercase();
    let truecolor_env = colorterm == "truecolor" || colorterm == "24bit";
    let tmux = env("TMUX").is_some();
    let dark_background = env("COLORFGBG").and_then(|v| parse_colorfgbg(&v));

    let mut caps = Caps {
        terminal: "unknown".into(),
        tier: Tier::Half,
        shapes: false,
        depth: ColorDepth::Ansi256,
        robust: false,
        underline_color: false,
        tmux,
        conhost: false,
        dark_background,
        hint: None,
    };

    if tmux {
        // The outer terminal is unknown; only COLORTERM says anything
        // about it (tmux passes it through when RGB is enabled).
        caps.terminal = "tmux".into();
        caps.depth = if truecolor_env {
            ColorDepth::True
        } else {
            caps.hint = Some(
                "tmux without truecolor: add `set -as terminal-features ',*:RGB'` to tmux.conf \
                 and set COLORTERM=truecolor"
                    .into(),
            );
            ColorDepth::Ansi256
        };
        caps.robust = true;
        return caps;
    }

    if let Some(p) = profile_for(env) {
        caps.terminal = p.name.into();
        caps.tier = p.tier;
        caps.depth = p.depth;
        caps.underline_color = p.underline_color;
        caps.shapes = p.tier >= Tier::Sextant;
    } else if windows {
        // Native console host: VT mode works (crossterm enables it), but the
        // console fonts only cover the basic block elements.
        caps.terminal = "conhost".into();
        caps.conhost = true;
        caps.tier = Tier::Half;
        caps.depth = ColorDepth::Ansi256;
    } else if term.contains("256color") {
        caps.terminal = term.clone();
    } else if term == "dumb" || term.is_empty() {
        caps.terminal = if term.is_empty() {
            "unknown".into()
        } else {
            term.clone()
        };
        caps.tier = Tier::Ascii;
        caps.depth = ColorDepth::Ansi16;
    } else {
        caps.terminal = term.clone();
    }

    if truecolor_env {
        caps.depth = ColorDepth::True;
    }
    caps.robust = caps.tier.needs_robust_output();
    caps
}

/// `COLORFGBG` is "fg;bg" (or "fg;default;bg") with palette indices.
fn parse_colorfgbg(v: &str) -> Option<bool> {
    let bg: u8 = v.rsplit(';').next()?.parse().ok()?;
    Some(!matches!(bg, 7 | 9..=15))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k: &str| owned.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone())
    }

    #[test]
    fn unknown_terminal_is_half_256() {
        let c = detect_from(&env_of(&[("TERM", "xterm-256color")]), false);
        assert_eq!((c.tier, c.depth, c.robust), (Tier::Half, ColorDepth::Ansi256, false));
    }

    #[test]
    fn tmux_needs_colorterm_for_truecolor() {
        let c = detect_from(&env_of(&[("TMUX", "/tmp/x"), ("TERM", "tmux-256color")]), false);
        assert_eq!(c.depth, ColorDepth::Ansi256);
        assert!(c.robust && c.hint.is_some());
        let c = detect_from(&env_of(&[("TMUX", "/tmp/x"), ("COLORTERM", "truecolor")]), false);
        assert_eq!(c.depth, ColorDepth::True);
        assert_eq!(c.tier, Tier::Half);
    }

    #[test]
    fn known_terminals() {
        let c = detect_from(&env_of(&[("TERM", "xterm-kitty")]), false);
        assert_eq!((c.tier, c.depth, c.robust), (Tier::Octant, ColorDepth::True, true));
        let c = detect_from(&env_of(&[("WT_SESSION", "x")]), true);
        assert_eq!((c.tier, c.depth, c.conhost), (Tier::Quadrant, ColorDepth::True, false));
    }

    #[test]
    fn conhost_without_windows_terminal() {
        let c = detect_from(&env_of(&[]), true);
        assert!(c.conhost);
        assert_eq!((c.tier, c.depth), (Tier::Half, ColorDepth::Ansi256));
    }

    #[test]
    fn colorfgbg() {
        assert_eq!(parse_colorfgbg("15;0"), Some(true));
        assert_eq!(parse_colorfgbg("0;15"), Some(false));
        assert_eq!(parse_colorfgbg("0;default;7"), Some(false));
        assert_eq!(parse_colorfgbg("x"), None);
    }
}
