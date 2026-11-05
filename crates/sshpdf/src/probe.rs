//! `--probe`: a test card for choosing the glyph tier and color depth.
//!
//! Each tier is drawn twice: a small picture (a circle, a thin diagonal and
//! a gradient) and a row of its glyphs followed by a `│`. If the terminal or
//! its font draws a glyph with the wrong width or not at all, the picture
//! breaks up and the `│` at the end of the row moves out of line.

use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use sshpdf_raster::{GlyphSet, Options as RasterOptions, SH, SW, Theme, optimize};
use sshpdf_term::{Attrs, Cell, CellGrid, Color, ColorDepth, Tier};

use crate::cli::Cli;
use crate::config::{self, Config, Settings};

const CARD_COLS: u16 = 80;
const PIC_COLS: usize = 22;
const PIC_ROWS: usize = 5;

/// The test picture: a ring, a thin diagonal line and a color gradient.
fn picture() -> Vec<u8> {
    let (w, h) = (PIC_COLS * SW, PIC_ROWS * SH);
    let mut img = vec![255u8; w * h * 4];
    let (cx, cy, r) = (h as f32 * 0.5, h as f32 * 0.5, h as f32 * 0.42);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let mut c = [255u8, 255, 255];
            if (d - r).abs() < 3.0 {
                c = [30, 60, 160];
            }
            // A thin diagonal from top-left to bottom-right of the right half.
            let rx = fx - w as f32 * 0.42;
            if rx >= 0.0 && (rx / (w as f32 * 0.58) - fy / h as f32).abs() * h as f32 <= 1.0 {
                c = [0, 0, 0];
            }
            // A hue gradient in the bottom quarter of the right half.
            if fy > h as f32 * 0.75 && rx > w as f32 * 0.1 {
                let t = (rx / (w as f32 * 0.48)).clamp(0.0, 1.0);
                c = [
                    (255.0 * (1.0 - t)) as u8,
                    (200.0 * (1.0 - (2.0 * t - 1.0).abs())) as u8,
                    (255.0 * t) as u8,
                ];
            }
            img[i..i + 3].copy_from_slice(&c);
        }
    }
    img
}

fn label(g: &mut CellGrid, col: u16, row: u16, text: &str, attrs: Attrs) {
    g.put_str(col, row, text, Cell::BLANK.with_attrs(attrs));
}

/// Build the card.
pub fn card(settings: &Settings) -> CellGrid {
    let tiers: Vec<(Tier, bool, &str)> = vec![
        (Tier::Ascii, false, "ascii"),
        (Tier::Half, false, "half"),
        (Tier::Quadrant, false, "quadrant"),
        (Tier::Sextant, false, "sextant"),
        (Tier::Octant, false, "octant"),
        (Tier::Octant, true, "octant+shapes"),
        (Tier::Braille, false, "braille"),
    ];
    let rows = 4 + tiers.len() as u16 * (PIC_ROWS as u16 + 1) + 9;
    let mut g = CellGrid::new(CARD_COLS, rows);
    let caps = &settings.caps;
    label(&mut g, 0, 0, "sshpdf --probe", Attrs::BOLD);
    label(
        &mut g,
        0,
        1,
        &format!(
            "terminal: {}{}   using: tier {}{}, colors {}",
            caps.terminal,
            if caps.tmux { " (in tmux)" } else { "" },
            caps.tier,
            if caps.shapes { "+shapes" } else { "" },
            caps.depth
        ),
        Attrs::NONE,
    );
    label(
        &mut g,
        0,
        2,
        "Pick the finest tier whose picture is clean and whose │ marks line up.",
        Attrs::DIM,
    );
    let img = picture();
    let theme = Theme {
        paper: false,
        dark: settings.dark,
        depth: ColorDepth::True,
    };
    let mut row = 4u16;
    for (tier, shapes, name) in tiers {
        let set = GlyphSet::get(tier, shapes);
        let cells = optimize(&img, PIC_COLS, PIC_ROWS, set, &RasterOptions::default());
        label(&mut g, 0, row, name, Attrs::BOLD);
        for (i, rc) in cells.iter().enumerate() {
            let (c, r) = (i % PIC_COLS, i / PIC_COLS);
            let mut cell = theme.raster_cell(
                &sshpdf_raster::RasterCell {
                    glyph: rc.glyph,
                    fg: theme.to_display(rc.fg),
                    bg: theme.to_display(rc.bg),
                },
                set,
                false,
            );
            if cell.ch == ' ' && cell.bg == Color::Default {
                cell = Cell::BLANK;
            }
            g.set(14 + c as u16, row + r as u16, cell);
        }
        // Glyph rows with an alignment mark after each.
        let glyphs: Vec<char> = set.glyphs.iter().map(|gl| gl.ch).filter(|c| *c != ' ').collect();
        let per_row = 40usize;
        for (k, chunk) in glyphs.chunks(per_row).take(PIC_ROWS).enumerate() {
            let s: String = chunk.iter().collect();
            let r = row + k as u16;
            g.put_str(38, r, &s, Cell::BLANK);
            g.put_str(38 + per_row as u16, r, "│", Cell::BLANK.with_attrs(Attrs::BOLD));
        }
        row += PIC_ROWS as u16 + 1;
    }
    // Color ramps, each written in its own encoding.
    label(&mut g, 0, row, "truecolor", Attrs::BOLD);
    for i in 0..64u16 {
        let t = i as f32 / 63.0;
        let rgb = hsv(t * 300.0, 0.85, 0.95);
        g.set(
            14 + i,
            row,
            Cell::new(' ', Color::Default, Color::Rgb(rgb[0], rgb[1], rgb[2])),
        );
        let v = (t * 255.0) as u8;
        g.set(14 + i, row + 1, Cell::new(' ', Color::Default, Color::Rgb(v, v, v)));
    }
    row += 3;
    label(&mut g, 0, row, "256 colors", Attrs::BOLD);
    for i in 0..64u16 {
        // A walk through the 6x6x6 cube, then the gray ramp.
        let idx = 16 + (i as u32 * 216 / 64) as u8;
        g.set(14 + i, row, Cell::new(' ', Color::Default, Color::Indexed(idx)));
        if i < 24 {
            g.set(
                14 + i,
                row + 1,
                Cell::new(' ', Color::Default, Color::Indexed(232 + i as u8)),
            );
        }
    }
    row += 3;
    label(&mut g, 0, row, "16 colors", Attrs::BOLD);
    for i in 0..16u16 {
        for k in 0..4 {
            g.set(
                14 + i * 4 + k,
                row,
                Cell::new(' ', Color::Default, Color::Indexed(i as u8)),
            );
        }
    }
    g
}

fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    ]
}

fn ask(question: &str, default: &str, valid: &[&str]) -> Result<String> {
    let stdin = io::stdin();
    loop {
        print!("{question} [{default}]: ");
        io::stdout().flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            return Ok(default.to_string());
        }
        let a = line.trim();
        if a.is_empty() {
            return Ok(default.to_string());
        }
        if valid.contains(&a) {
            return Ok(a.to_string());
        }
        println!("  one of: {}", valid.join(", "));
    }
}

/// `--probe [--save]`.
pub fn run(cli: &Cli, settings: &Settings, mut config: Config) -> Result<()> {
    let g = card(settings);
    let stdout = io::stdout();
    {
        let mut out = stdout.lock();
        // True depth: each ramp keeps the encoding it tests. No robust mode:
        // misaligned glyphs should show.
        sshpdf_term::sgr::write_grid(&mut out, &g, ColorDepth::True, false)?;
        out.flush()?;
    }
    if !cli.save {
        println!("\nTo keep a choice: sshpdf --probe --save (or set tier/colors in the config file).");
        return Ok(());
    }
    let tiers = [
        "ascii",
        "half",
        "quadrant",
        "sextant",
        "octant",
        "octant+shapes",
        "braille",
    ];
    let current = format!(
        "{}{}",
        settings.caps.tier,
        if settings.caps.shapes && settings.caps.tier >= Tier::Sextant {
            "+shapes"
        } else {
            ""
        }
    );
    let tier = ask("\nTier", &current, &tiers)?;
    let colors = ask(
        "Colors (true, 256, 16)",
        settings.caps.depth.name(),
        &["true", "256", "16"],
    )?;
    let (t, shapes) = match tier.split_once('+') {
        Some((t, _)) => (t.to_string(), true),
        None => (tier.clone(), false),
    };
    config.tier = Some(t);
    config.shapes = Some(shapes);
    config.colors = Some(colors);
    let path = cli
        .config
        .clone()
        .or_else(config::default_path)
        .context("no config directory")?;
    config.save(&path)?;
    println!("saved to {}", path.display());
    Ok(())
}
