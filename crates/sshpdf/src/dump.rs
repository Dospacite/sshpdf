//! One-shot output without the TUI: `--dump`, `--dump-png`, `--text`.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use sshpdf_doc::OpenError;
use sshpdf_layout::{CellRect, render_cells};
use sshpdf_term::CellGrid;

use crate::cli::{Cli, parse_size};
use crate::config::{Settings, ViewMode};
use crate::session::Session;

/// Whether `SSHPDF_DEBUG` (a comma-separated list) names `what`. Prints
/// details to stderr in `--dump`: `blocks` (text blocks in reading order),
/// `sizes` (text lines and font sizes),
/// `images` (photo classification), `links`, `dropped` (text left to the
/// raster layer).
fn debug(what: &str) -> bool {
    std::env::var("SSHPDF_DEBUG").is_ok_and(|v| v.split(',').any(|w| w.trim() == what))
}

/// Open the file named on the command line.
pub fn open(cli: &Cli) -> Result<Session> {
    let path = cli.file.as_deref().context("no FILE given (see --help)")?;
    open_path(path, cli.password.as_deref())
}

pub fn open_path(path: &Path, password: Option<&str>) -> Result<Session> {
    match Session::open(path, password) {
        Ok(s) => Ok(s),
        Err(OpenError::PasswordRequired) => bail!("{} is encrypted: pass --password", path.display()),
        Err(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
    }
}

/// The page index (0-based) from `--page` (1-based).
fn page_index(cli: &Cli, s: &Session) -> Result<usize> {
    let page = cli.page.unwrap_or(1);
    if page == 0 || page > s.page_count() {
        bail!(
            "page {page} is out of range (the document has {} pages)",
            s.page_count()
        );
    }
    Ok(page - 1)
}

/// Render the cells `--dump` / `--dump-png` show.
pub fn dump_grid(cli: &Cli, settings: &Settings, session: &Session) -> Result<CellGrid> {
    let page = page_index(cli, session)?;
    let (cols, rows) = parse_size(&cli.size).context("--size must look like 120x40")?;
    if settings.mode == ViewMode::Reflow {
        let text = session.text(page);
        let lines = sshpdf_layout::reflow::reflow(&text, page, cols as usize, &session.graphics(page));
        let rows = if rows == 0 { lines.len().max(1) as u16 } else { rows };
        let start = cli.scroll.unwrap_or(0).max(0) as usize;
        return Ok(sshpdf_layout::reflow::to_grid(
            &lines[start.min(lines.len())..],
            cols,
            rows,
            &settings.theme(),
        ));
    }
    let mapping = session.mapping(
        settings.zoom,
        settings.mode,
        page,
        cols,
        rows.max(1),
        settings.cell_aspect,
    );
    let (pc, pr) = mapping.page_cells(session.page_size(page));
    let rows = if rows == 0 {
        pr.min(u16::MAX as u32) as u16
    } else {
        rows
    };
    // Centre a page narrower than the output, like the viewer does.
    let col0 = if pc < cols as u32 {
        -(((cols as u32 - pc) / 2) as i32)
    } else {
        0
    };
    let row0 = cli.scroll.unwrap_or(0);
    let layout = session.layout(page, mapping, settings.mode);
    if debug("blocks") {
        for b in &session.text(page).blocks {
            let first = b.lines.first().map(|l| l.text()).unwrap_or_default();
            eprintln!(
                "block x {:6.1}-{:6.1} y {:6.1}-{:6.1} {:.50}",
                b.bbox.x0, b.bbox.x1, b.bbox.y0, b.bbox.y1, first
            );
        }
    }
    if debug("sizes") {
        for l in session.text(page).lines() {
            eprintln!(
                "size {:5.1} y {:6.1} x {:6.1} {}",
                l.size,
                l.baseline,
                l.bbox.x0,
                l.text()
            );
        }
    }
    if debug("images") {
        let photos = session.photos(page);
        for r in &session.graphics(page).images {
            eprintln!("image {r:?} photo={}", photos.contains(r));
        }
    }
    if debug("links") {
        for l in session.doc.links(page) {
            eprintln!("link {:?} -> {:?}", l.rect, l.target);
        }
    }
    if debug("dropped") {
        eprintln!("mapping {mapping:?}");
        for (r, row) in &layout.text.dropped {
            eprintln!("dropped row {row}: {r:?}");
        }
    }
    Ok(render_cells(
        &*session.doc,
        &layout,
        CellRect::new(col0, row0, cols, rows),
        &settings.render_options(),
    ))
}

/// `--dump`: ANSI text on stdout.
pub fn dump(cli: &Cli, settings: &Settings) -> Result<()> {
    let session = open(cli)?;
    let grid = dump_grid(cli, settings, &session)?;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    sshpdf_term::sgr::write_grid(&mut out, &grid, settings.caps.depth, cli.robust)?;
    out.flush()?;
    Ok(())
}

/// `--dump-png`: a simulated terminal render.
pub fn dump_png(cli: &Cli, settings: &Settings) -> Result<()> {
    let output = cli.output.as_deref().context("--dump-png needs -o FILE")?;
    let session = open(cli)?;
    let grid = dump_grid(cli, settings, &session)?;
    let cell_h = 20u32;
    let cell_w = ((cell_h as f32 * settings.cell_aspect).round() as u32).max(4);
    let (w, h, rgb) = crate::pngout::render(&grid, cell_w, cell_h, crate::pngout::PngTheme::new(settings.dark));
    crate::pngout::write_png(output, w, h, &rgb)
}

/// `--text`: reflowed plain text of one page or the whole document.
pub fn text(cli: &Cli) -> Result<()> {
    let session = open(cli)?;
    let pages: Vec<usize> = match cli.page {
        Some(_) => vec![page_index(cli, &session)?],
        None => (0..session.page_count()).collect(),
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for (k, &p) in pages.iter().enumerate() {
        if k > 0 {
            writeln!(out, "\u{c}")?;
        }
        let text = session.text(p);
        let lines = sshpdf_layout::reflow::reflow(&text, p, 80, &session.graphics(p));
        for line in &lines {
            writeln!(out, "{}", line.plain())?;
        }
    }
    out.flush()?;
    Ok(())
}

/// `--render-ansi`: draw ANSI text as a PNG (to check captures by eye).
pub fn render_ansi(cli: &Cli, settings: &Settings, input: &Path) -> Result<()> {
    let output = cli.output.as_deref().context("--render-ansi needs -o FILE")?;
    let text = std::fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;
    let (cols, _) = parse_size(&cli.size).context("--size must look like 120x40")?;
    let grid = crate::ansi::parse(&text, cols);
    let cell_h = 20u32;
    let cell_w = ((cell_h as f32 * settings.cell_aspect).round() as u32).max(4);
    let (w, h, rgb) = crate::pngout::render(&grid, cell_w, cell_h, crate::pngout::PngTheme::new(settings.dark));
    crate::pngout::write_png(output, w, h, &rgb)
}
