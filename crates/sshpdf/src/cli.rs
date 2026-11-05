//! Command-line interface.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug, Clone, Default)]
#[command(
    name = "sshpdf",
    version,
    about = "A PDF viewer for any terminal, drawn with characters only",
    after_help = "Examples:\n  \
        sshpdf paper.pdf\n  \
        sshpdf paper.pdf --page 3 --mode reflow\n  \
        sshpdf --dump paper.pdf --page 1 --size 120x40 --tier octant\n  \
        sshpdf --dump-png paper.pdf --page 1 --size 120x40 -o out.png\n  \
        sshpdf --text paper.pdf --page 2\n  \
        sshpdf --probe --save"
)]
pub struct Cli {
    /// The PDF file.
    pub file: Option<PathBuf>,

    /// Page to open (1-based).
    #[arg(long, short = 'p', value_name = "N")]
    pub page: Option<usize>,

    /// layout (text as characters), reflow (text only, wrapped) or raster.
    #[arg(long, value_name = "MODE")]
    pub mode: Option<String>,

    /// Glyph set for images: ascii, half, quadrant, sextant, octant, braille.
    #[arg(long, value_name = "TIER")]
    pub tier: Option<String>,

    /// Add Legacy Computing wedges, shades and eighth blocks (sextant/octant).
    #[arg(long, overrides_with = "no_shapes")]
    pub shapes: bool,
    #[arg(long, hide = true)]
    pub no_shapes: bool,

    /// Color depth: true, 256 or 16.
    #[arg(long, value_name = "DEPTH")]
    pub colors: Option<String>,

    /// text (one row per text line), width, page, or a percentage (100 = 12 pt per row).
    #[arg(long, value_name = "ZOOM")]
    pub zoom: Option<String>,

    /// Draw white paper instead of blending into the terminal background.
    #[arg(long)]
    pub paper: bool,

    /// Assume a dark or light terminal background (for colors on the page).
    #[arg(long, value_name = "dark|light")]
    pub theme: Option<String>,

    /// Don't reposition the cursor after characters of uncertain width.
    #[arg(long, overrides_with = "robust")]
    pub no_robust: bool,
    /// Always reposition the cursor after characters of uncertain width.
    #[arg(long)]
    pub robust: bool,

    /// Turn off error diffusion between cells.
    #[arg(long)]
    pub no_diffusion: bool,

    /// Cell width / height of the terminal font (default 0.5).
    #[arg(long, value_name = "RATIO")]
    pub cell_aspect: Option<f32>,

    /// Password for an encrypted PDF.
    #[arg(long, value_name = "PASSWORD")]
    pub password: Option<String>,

    /// Show a test card of glyph tiers and colors to pick what works.
    #[arg(long)]
    pub probe: bool,
    /// With --probe: save the chosen settings to the config file.
    #[arg(long, requires = "probe")]
    pub save: bool,

    /// Write the page as ANSI text to stdout (no TUI).
    #[arg(long, conflicts_with_all = ["dump_png", "text", "probe"])]
    pub dump: bool,
    /// Write a simulated terminal render of the page as PNG (needs -o).
    #[arg(long, conflicts_with_all = ["text", "probe"])]
    pub dump_png: bool,
    /// Write the reflowed text of the document (or one page) to stdout.
    #[arg(long, conflicts_with = "probe")]
    pub text: bool,

    /// Size for --dump/--dump-png, COLSxROWS (ROWS 0 = the whole page).
    #[arg(long, default_value = "120x40", value_name = "COLSxROWS")]
    pub size: String,
    /// First row of the page grid to dump (default 0).
    #[arg(long, value_name = "ROW", allow_hyphen_values = true)]
    pub scroll: Option<i32>,
    /// Output file for --dump-png.
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Developer aid: render ANSI text (a --dump or `tmux capture-pane -e`
    /// file) to the PNG given with -o, COLS wide (from --size).
    #[arg(long, value_name = "FILE", hide = true)]
    pub render_ansi: Option<PathBuf>,

    /// Config file (default: the config dir's sshpdf/config.toml).
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// Write a debug log to this file.
    #[arg(long, value_name = "FILE")]
    pub log: Option<PathBuf>,
}

/// Parse "COLSxROWS".
pub fn parse_size(s: &str) -> Option<(u16, u16)> {
    let (c, r) = s.split_once(['x', 'X'])?;
    let c: u16 = c.trim().parse().ok()?;
    let r: u16 = r.trim().parse().ok()?;
    (c > 0).then_some((c, r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("120x40"), Some((120, 40)));
        assert_eq!(parse_size("80X0"), Some((80, 0)));
        assert_eq!(parse_size("0x10"), None);
        assert_eq!(parse_size("abc"), None);
    }

    #[test]
    fn cli_parses() {
        Cli::command_for_test();
        let c = Cli::try_parse_from(["sshpdf", "--dump", "a.pdf", "--page", "2", "--size", "80x24"]).unwrap();
        assert!(c.dump);
        assert_eq!(c.page, Some(2));
        assert!(Cli::try_parse_from(["sshpdf", "--dump", "--text", "a.pdf"]).is_err());
    }

    impl Cli {
        fn command_for_test() {
            use clap::CommandFactory;
            Cli::command().debug_assert();
        }
    }
}
