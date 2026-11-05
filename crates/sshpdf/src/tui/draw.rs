//! Drawing a frame: the page view, the lens, panels and the status line.

use std::collections::HashSet;
use std::time::Duration;

use sshpdf_layout::reflow::LineKind;
use sshpdf_layout::{CellRect, render_cells};
use sshpdf_term::backend::to_ratatui_cell;
use sshpdf_term::{Attrs, Cell, CellGrid, Color};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::app::{App, Overlay};
use super::keys::Action;
use super::tiles::{TILE_COLS, TILE_ROWS, TileKey, ViewKey};
use crate::config::{ViewMode, ZoomSpec};

/// How long a status message stays.
pub const MESSAGE_TIME: Duration = Duration::from_secs(4);

fn put_grid(buf: &mut Buffer, grid: &CellGrid, x: u16, y: u16) {
    for r in 0..grid.rows() {
        for c in 0..grid.cols() {
            let (bx, by) = (x + c, y + r);
            if bx >= buf.area.width || by >= buf.area.height {
                continue;
            }
            if let Some(cell) = buf.cell_mut((bx, by)) {
                to_ratatui_cell(grid.get(c, r).expect("in bounds"), cell);
            }
        }
    }
}

/// The page view from cached tiles; returns how many tiles were missing.
fn page_grid(app: &mut App) -> (CellGrid, usize) {
    let w = app.window();
    let mut grid = CellGrid::new(w.cols, w.rows);
    let view = app.view_key();
    let (pc, pr) = app.page_cells();
    let mut missing = 0;
    let tx0 = (w.col.max(0) / TILE_COLS).max(0);
    let tx1 = ((w.col + w.cols as i32 - 1).min(pc - 1)).max(0) / TILE_COLS;
    let ty0 = (w.row.max(0) / TILE_ROWS).max(0);
    let ty1 = ((w.row + w.rows as i32 - 1).min(pr - 1)).max(0) / TILE_ROWS;
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let key = TileKey { view, tx, ty };
            let rect = key.rect(pc, pr);
            match app.tiles.get(&key) {
                Some(tile) => grid.blit(&tile, rect.col - w.col, rect.row - w.row),
                None => missing += 1,
            }
        }
    }
    // Search matches on this page.
    if app.mode == ViewMode::Layout && !app.search.matches.is_empty() {
        let mut all = HashSet::new();
        let mut current = HashSet::new();
        for (i, (p, m)) in app.search.matches.iter().enumerate() {
            if *p != app.page {
                continue;
            }
            all.extend(m.chars.iter().copied());
            if app.search.current == Some(i) {
                current.extend(m.chars.iter().copied());
            }
        }
        if !all.is_empty() {
            let layout = app.tiles.layout(view);
            for (&(col, row), tc) in &layout.text.cells {
                if all.contains(&tc.src) && w.contains(col, row) {
                    let cell = grid
                        .get_mut((col - w.col) as u16, (row - w.row) as u16)
                        .expect("in window");
                    cell.attrs |= Attrs::REVERSE;
                    if current.contains(&tc.src) {
                        cell.attrs |= Attrs::BOLD | Attrs::UNDERLINE;
                    }
                }
            }
        }
    }
    // The focused link.
    if let Some(f) = app.link_focus {
        let links = app.visible_links();
        if let Some((_, r)) = links.get(f) {
            for row in r.row.max(0)..(r.row + r.rows as i32).min(w.rows as i32) {
                for col in r.col.max(0)..(r.col + r.cols as i32).min(w.cols as i32) {
                    if let Some(cell) = grid.get_mut(col as u16, row as u16) {
                        cell.attrs |= Attrs::REVERSE;
                    }
                }
            }
        }
    }
    (grid, missing)
}

/// The reflow view: lines of the current page and the ones after it.
fn reflow_grid(app: &mut App) -> CellGrid {
    let (cols, rows) = (app.view_cols, app.view_rows);
    let width = app.reflow_width();
    let left = (cols.saturating_sub(width)) / 2;
    let mut grid = CellGrid::new(cols, rows);
    let theme = app.settings.theme();
    let mut pos = app.reflow;
    let mut r = 0u16;
    let pages = app.session.page_count();
    let current: HashSet<u32> = app
        .search
        .current
        .and_then(|i| app.search.matches.get(i))
        .map(|(_, m)| m.chars.iter().copied().collect())
        .unwrap_or_default();
    let current_page = app
        .search
        .current
        .and_then(|i| app.search.matches.get(i))
        .map(|(p, _)| *p);
    while r < rows && pos.page < pages {
        let lines = app.reflow_lines(pos.page);
        if pos.line < lines.len() {
            let line = &lines[pos.line];
            let one = sshpdf_layout::reflow::to_grid(std::slice::from_ref(line), width, 1, &theme);
            let mut row = one;
            if current_page == Some(pos.page) {
                for (k, tc) in line.cells.iter().enumerate() {
                    if current.contains(&tc.src)
                        && let Some(c) = row.get_mut(k as u16, 0)
                    {
                        c.attrs |= Attrs::REVERSE | Attrs::BOLD;
                    }
                }
            }
            if matches!(line.kind, LineKind::Figure { .. }) {
                for k in 0..row.cols() {
                    if let Some(c) = row.get_mut(k, 0) {
                        c.attrs |= Attrs::DIM;
                    }
                }
            }
            grid.blit(&row, left as i32, r as i32);
            pos.line += 1;
        } else {
            // Page separator.
            let label = format!(" page {} ", pos.page + 2);
            if pos.page + 1 < pages {
                let rule = "─".repeat(width as usize);
                grid.put_str(left, r, &rule, Cell::new(' ', Color::Indexed(8), Color::Default));
                let x = left + (width.saturating_sub(label.width() as u16)) / 2;
                grid.put_str(x, r, &label, Cell::new(' ', Color::Indexed(8), Color::Default));
            }
            pos.page += 1;
            pos.line = 0;
        }
        r += 1;
    }
    grid
}

/// The lens content, framed.
fn lens_grid(app: &mut App) -> Option<(CellGrid, u16, u16)> {
    let lens = app.lens?;
    let (bw, bh) = (
        app.settings.lens_cols.min(app.view_cols),
        app.settings.lens_rows.min(app.view_rows),
    );
    if bw < 6 || bh < 4 {
        return None;
    }
    let (iw, ih) = (bw - 2, bh - 2);
    let lm = app.mapping.zoomed(lens.magnification);
    let lm = if app.mode == ViewMode::Raster {
        lm.with_stretch(1.0)
    } else {
        lm
    };
    let (px, py) = app.mapping.to_page(lens.col as f32 + 0.5, lens.row as f32 + 0.5);
    let (cc, cr) = lm.to_cell(px, py);
    let win = CellRect::new(
        (cc - iw as f32 / 2.0).round() as i32,
        (cr - ih as f32 / 2.0).round() as i32,
        iw,
        ih,
    );
    let layout = app.tiles.layout(ViewKey::new(app.page, &lm, app.mode));
    let inner = render_cells(&*app.session.doc, &layout, win, &app.settings.render_options());
    let mut g = CellGrid::new(bw, bh);
    let frame = Cell::new(' ', Color::Default, Color::Default).with_attrs(Attrs::BOLD);
    let title = format!(" lens {:.0}× ", lens.magnification);
    g.put_str(0, 0, &format!("┏{}┓", "━".repeat(iw as usize)), frame);
    g.put_str(2, 0, &title, frame);
    for r in 1..bh - 1 {
        g.set(0, r, Cell { ch: '┃', ..frame });
        g.set(bw - 1, r, Cell { ch: '┃', ..frame });
    }
    g.put_str(0, bh - 1, &format!("┗{}┛", "━".repeat(iw as usize)), frame);
    g.blit(&inner, 1, 1);
    // Centre the box on the lens position, kept on screen.
    let sc = lens.col - app.scroll_col;
    let sr = lens.row - app.scroll_row;
    let x = (sc - bw as i32 / 2).clamp(0, (app.view_cols - bw) as i32) as u16;
    let y = (sr - bh as i32 / 2).clamp(0, (app.view_rows - bh) as i32) as u16;
    Some((g, x, y))
}

fn zoom_label(app: &App) -> String {
    let pct = app.mapping.zoom_percent().round();
    match app.zoom {
        ZoomSpec::Text => format!("text {pct}%"),
        ZoomSpec::Width => format!("width {pct}%"),
        ZoomSpec::Page => format!("page {pct}%"),
        ZoomSpec::Percent(_) => format!("{pct}%"),
    }
}

fn status_line(app: &mut App, missing: usize, width: u16) -> String {
    let n = app.session.page_count();
    let shapes = if app.settings.caps.shapes && app.settings.caps.tier >= sshpdf_term::Tier::Sextant {
        "+shapes"
    } else {
        ""
    };
    let left = format!(
        " {}  {}/{}  {}  {}  {}{}",
        app.session.file_name(),
        app.page + 1,
        n,
        zoom_label(app),
        app.mode.name(),
        app.settings.caps.tier,
        shapes
    );
    let right = if let Some((msg, at)) = &app.message
        && at.elapsed() < MESSAGE_TIME
    {
        msg.clone()
    } else if missing > 0 {
        "rendering…".to_string()
    } else if !app.legible() {
        "text too small here: zoom in (+) or press r for reflow".to_string()
    } else {
        "? help".to_string()
    };
    let lw = left.width();
    let rw = right.width();
    let w = width as usize;
    if lw + rw + 2 <= w {
        format!("{left}{}{right} ", " ".repeat(w - lw - rw - 1))
    } else {
        let mut s = format!("{left}  {right}");
        while s.width() > w {
            s.pop();
        }
        s
    }
}

fn help_text(app: &App) -> String {
    let mut s = String::new();
    for (action, _, _) in Action::ALL {
        let keys = app.keymap.keys_for(action);
        if keys.is_empty() {
            continue;
        }
        s.push_str(&format!(
            "{:<16} {:<44} {}\n",
            keys.join(" "),
            action.describe(),
            action.name()
        ));
    }
    s.push_str("\nA number before a key repeats it or picks a page: 12G goes to page 12.\n");
    s.push_str("The right column names the action for the [keys] section of the config file.\n");
    s
}

fn info_text(app: &mut App) -> String {
    let caps = &app.settings.caps;
    let m = app.mapping;
    let (tiles, avg, last, cached) = app.tiles.stats();
    let size = app.session.page_size(app.page);
    let title = app.session.doc.title().unwrap_or_else(|| "-".into());
    format!(
        "file      {}\ntitle     {}\npage      {} of {}  ({:.0} x {:.0} pt)\nzoom      {}  ({:.2} pt/row, stretch {:.2}x)\nmode      {}\ntier      {}{}\ncolors    {}{}\nterminal  {}{}\nrobust    {}\ntheme     {}\nframe     {} bytes, {:.1} ms\ntiles     {} rendered, {:.1} ms avg, {:.1} ms last, {} cached, {} queued\ntext      {} chars on this page\n",
        app.session.path.display(),
        title,
        app.page + 1,
        app.session.page_count(),
        size.w,
        size.h,
        zoom_label(app),
        m.row_pitch,
        m.x_stretch,
        app.mode.name(),
        caps.tier,
        if caps.shapes { " +shapes" } else { "" },
        caps.depth,
        if caps.depth == sshpdf_term::ColorDepth::True {
            " (truecolor)"
        } else {
            ""
        },
        caps.terminal,
        if caps.tmux { " (in tmux)" } else { "" },
        if caps.robust { "on" } else { "off" },
        if app.settings.paper {
            "paper"
        } else if app.settings.dark {
            "dark terminal"
        } else {
            "light terminal"
        },
        app.last_frame_bytes,
        app.last_frame_ms,
        tiles,
        avg,
        last,
        cached,
        app.tiles.pending(),
        app.session.text(app.page).char_count(),
    ) + &match &caps.hint {
        Some(h) => format!("\nhint: {h}\n"),
        None => String::new(),
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

/// Draw the whole frame. Returns the number of missing tiles.
pub fn draw(f: &mut Frame<'_>, app: &mut App) -> usize {
    let area = f.area();
    if area.height < 2 || area.width < 10 {
        return 0;
    }
    let view = Rect::new(0, 0, area.width, area.height - 1);
    let (grid, missing) = if app.mode == ViewMode::Reflow {
        (reflow_grid(app), 0)
    } else {
        page_grid(app)
    };
    put_grid(f.buffer_mut(), &grid, view.x, view.y);
    if let Some((g, x, y)) = lens_grid(app) {
        put_grid(f.buffer_mut(), &g, x, y);
    }

    if let Some(panel) = app.outline.as_mut() {
        let w = (area.width / 2).clamp(20, 60);
        let rect = Rect::new(0, 0, w, view.height);
        let inner_h = rect.height.saturating_sub(2) as usize;
        if panel.selected < panel.scroll {
            panel.scroll = panel.selected;
        } else if panel.selected >= panel.scroll + inner_h.max(1) {
            panel.scroll = panel.selected + 1 - inner_h.max(1);
        }
        let lines: Vec<ratatui::text::Line<'_>> = panel
            .items
            .iter()
            .enumerate()
            .skip(panel.scroll)
            .take(inner_h)
            .map(|(i, super::app::OutlineEntry { depth, title, dest })| {
                let page = dest.map_or(String::new(), |(p, _)| format!(" {}", p + 1));
                let text = format!("{}{}", "  ".repeat(*depth), title);
                let room = (w as usize).saturating_sub(4 + page.width());
                let mut t: String = text.chars().take(room).collect();
                while t.width() > room {
                    t.pop();
                }
                let pad = room.saturating_sub(t.width());
                let s = format!("{t}{}{page}", " ".repeat(pad));
                let style = if i == panel.selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                ratatui::text::Line::styled(s, style)
            })
            .collect();
        f.render_widget(Clear, rect);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" outline: Enter jumps, Esc closes "),
            ),
            rect,
        );
    }

    match app.overlay {
        Overlay::Help { scroll } => {
            let text = help_text(app);
            let h = (text.lines().count() as u16 + 2).min(view.height);
            let rect = centered(view, 84, h);
            f.render_widget(Clear, rect);
            f.render_widget(
                Paragraph::new(text).scroll((scroll as u16, 0)).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" sshpdf keys (any key closes) "),
                ),
                rect,
            );
        }
        Overlay::Info => {
            let text = info_text(app);
            let h = (text.lines().count() as u16 + 2).min(view.height);
            let rect = centered(view, 90, h);
            f.render_widget(Clear, rect);
            f.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(" info ")),
                rect,
            );
        }
        Overlay::None => {}
    }

    let status_rect = Rect::new(0, area.height - 1, area.width, 1);
    let status = match &app.prompt {
        Some(p) => format!("/{}█", p.text),
        None => status_line(app, missing, area.width),
    };
    f.render_widget(
        Paragraph::new(status).style(Style::default().add_modifier(Modifier::REVERSED)),
        status_rect,
    );
    missing
}
