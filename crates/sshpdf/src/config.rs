//! The config file and how settings are resolved: CLI flags override the
//! config file, which overrides what was detected.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sshpdf_term::{Caps, ColorDepth, Tier};
use serde::{Deserialize, Serialize};

use crate::cli::Cli;

/// Contents of `config.toml`. Every field is optional.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// ascii, half, quadrant, sextant, octant, braille
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shapes: Option<bool>,
    /// true, 256, 16
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_aspect: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper: Option<bool>,
    /// dark or light
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robust: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffusion: Option<bool>,
    /// layout, reflow, raster
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// text, width, page or a percentage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens: Option<LensConfig>,
    /// Extra key bindings: action name → keys, e.g. `next_page = ["J", "ctrl-n"]`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LensConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magnification: Option<f32>,
}

/// Default location of the config file.
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sshpdf").join("config.toml"))
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        toml::from_str(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }
}

/// How a page is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewMode {
    Layout,
    Reflow,
    Raster,
}

impl ViewMode {
    pub fn parse(s: &str) -> Option<ViewMode> {
        match s {
            "layout" => Some(ViewMode::Layout),
            "reflow" => Some(ViewMode::Reflow),
            "raster" => Some(ViewMode::Raster),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ViewMode::Layout => "layout",
            ViewMode::Reflow => "reflow",
            ViewMode::Raster => "raster",
        }
    }
}

/// How the zoom is chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomSpec {
    /// One row per line of body text.
    Text,
    /// Page width fills the screen.
    Width,
    /// Whole page on screen.
    Page,
    /// 100 = 12 points per row.
    Percent(f32),
}

impl ZoomSpec {
    pub fn parse(s: &str) -> Option<ZoomSpec> {
        match s {
            "text" => Some(ZoomSpec::Text),
            "width" => Some(ZoomSpec::Width),
            "page" => Some(ZoomSpec::Page),
            _ => {
                let p: f32 = s.trim_end_matches('%').parse().ok()?;
                (p > 0.0).then_some(ZoomSpec::Percent(p))
            }
        }
    }
}

/// The resolved settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub caps: Caps,
    pub paper: bool,
    pub dark: bool,
    pub cell_aspect: f32,
    pub diffusion: bool,
    pub mode: ViewMode,
    pub zoom: ZoomSpec,
    pub lens_cols: u16,
    pub lens_rows: u16,
    pub lens_magnification: f32,
    pub keys: BTreeMap<String, Vec<String>>,
}

fn parse_tier(s: &str) -> Result<Tier> {
    match Tier::parse(s) {
        Some(t) => Ok(t),
        None => bail!("unknown tier {s:?} (ascii, half, quadrant, sextant, octant, braille)"),
    }
}

fn parse_depth(s: &str) -> Result<ColorDepth> {
    match ColorDepth::parse(s) {
        Some(d) => Ok(d),
        None => bail!("unknown color depth {s:?} (true, 256, 16)"),
    }
}

fn parse_dark(s: &str) -> Result<bool> {
    match s {
        "dark" => Ok(true),
        "light" => Ok(false),
        _ => bail!("unknown theme {s:?} (dark, light)"),
    }
}

impl Settings {
    /// Resolve: CLI flags, then the config file, then detection.
    pub fn resolve(cli: &Cli, config: &Config, detected: Caps) -> Result<Settings> {
        let mut caps = detected;
        let robust_was_auto = caps.robust == caps.tier.needs_robust_output() || caps.tmux;
        if let Some(t) = cli.tier.as_deref().or(config.tier.as_deref()) {
            caps.tier = parse_tier(t)?;
        }
        if let Some(d) = cli.colors.as_deref().or(config.colors.as_deref()) {
            caps.depth = parse_depth(d)?;
        }
        caps.shapes = if cli.shapes {
            true
        } else if cli.no_shapes {
            false
        } else {
            config.shapes.unwrap_or(caps.shapes)
        };
        caps.robust = if cli.robust {
            true
        } else if cli.no_robust {
            false
        } else if let Some(r) = config.robust {
            r
        } else if robust_was_auto {
            caps.tmux || caps.tier.needs_robust_output()
        } else {
            caps.robust
        };
        let dark = match cli.theme.as_deref().or(config.theme.as_deref()) {
            Some(t) => parse_dark(t)?,
            None => caps.dark_background.unwrap_or(true),
        };
        let mode = match cli.mode.as_deref().or(config.mode.as_deref()) {
            Some(m) => ViewMode::parse(m).with_context(|| format!("unknown mode {m:?} (layout, reflow, raster)"))?,
            None => ViewMode::Layout,
        };
        let zoom = match cli.zoom.as_deref().or(config.zoom.as_deref()) {
            Some(z) => ZoomSpec::parse(z)
                .with_context(|| format!("unknown zoom {z:?} (text, width, page, or a percentage)"))?,
            None => ZoomSpec::Text,
        };
        let cell_aspect = cli.cell_aspect.or(config.cell_aspect).unwrap_or(0.5);
        if !(0.2..=2.0).contains(&cell_aspect) {
            bail!("cell_aspect must be between 0.2 and 2.0");
        }
        let lens = config.lens.unwrap_or_default();
        Ok(Settings {
            caps,
            paper: cli.paper || config.paper.unwrap_or(false),
            dark,
            cell_aspect,
            diffusion: !cli.no_diffusion && config.diffusion.unwrap_or(true),
            mode,
            zoom,
            lens_cols: lens.cols.unwrap_or(40).clamp(10, 400),
            lens_rows: lens.rows.unwrap_or(12).clamp(4, 200),
            lens_magnification: lens.magnification.unwrap_or(4.0).clamp(2.0, 8.0),
            keys: config.keys.clone(),
        })
    }

    pub fn theme(&self) -> sshpdf_raster::Theme {
        sshpdf_raster::Theme {
            paper: self.paper,
            dark: self.dark,
            depth: self.caps.depth,
        }
    }

    pub fn render_options(&self) -> sshpdf_layout::RenderOptions {
        sshpdf_layout::RenderOptions {
            tier: self.caps.tier,
            shapes: self.caps.shapes,
            theme: self.theme(),
            diffusion: self.diffusion,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn detected() -> Caps {
        sshpdf_term::caps::detect_from(&|k| (k == "TERM").then(|| "xterm-256color".to_string()), false)
    }

    #[test]
    fn cli_beats_config_beats_detection() {
        let cli = Cli::try_parse_from(["sshpdf", "x.pdf", "--tier", "octant"]).unwrap();
        let config: Config = toml::from_str("tier = \"quadrant\"\ncolors = \"16\"\npaper = true").unwrap();
        let s = Settings::resolve(&cli, &config, detected()).unwrap();
        assert_eq!(s.caps.tier, Tier::Octant);
        assert_eq!(s.caps.depth, ColorDepth::Ansi16);
        assert!(s.paper);
        // Octant turns robust output on automatically.
        assert!(s.caps.robust);
        let s = Settings::resolve(&Cli::default(), &Config::default(), detected()).unwrap();
        assert_eq!((s.caps.tier, s.caps.depth), (Tier::Half, ColorDepth::Ansi256));
        assert_eq!(s.zoom, ZoomSpec::Text);
    }

    #[test]
    fn bad_values_are_errors() {
        let cli = Cli::try_parse_from(["sshpdf", "--tier", "hexant"]).unwrap();
        assert!(Settings::resolve(&cli, &Config::default(), detected()).is_err());
        assert!(toml::from_str::<Config>("tierr = \"x\"").is_err());
    }

    #[test]
    fn config_round_trip() {
        let c = Config {
            tier: Some("octant".into()),
            colors: Some("true".into()),
            lens: Some(LensConfig {
                cols: Some(50),
                ..Default::default()
            }),
            ..Default::default()
        };
        let text = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), c);
    }

    #[test]
    fn zoom_specs() {
        assert_eq!(ZoomSpec::parse("150"), Some(ZoomSpec::Percent(150.0)));
        assert_eq!(ZoomSpec::parse("80%"), Some(ZoomSpec::Percent(80.0)));
        assert_eq!(ZoomSpec::parse("width"), Some(ZoomSpec::Width));
        assert_eq!(ZoomSpec::parse("-5"), None);
    }
}
