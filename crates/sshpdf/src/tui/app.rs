//! The viewer's state and what each action does to it.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use sshpdf_doc::{Link, LinkTarget, OutlineItem};
use sshpdf_layout::reflow::{LineKind, RLine};
use sshpdf_layout::search::Match;
use sshpdf_layout::{CellRect, Mapping};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};

use super::keys::{Action, Key, Keymap};
use super::tiles::{TILE_COLS, TILE_ROWS, TileKey, Tiles, ViewKey};
use crate::config::{Settings, ViewMode, ZoomSpec};
use crate::session::Session;

/// What covers the page view.
#[derive(Clone, Debug, PartialEq)]
pub enum Overlay {
    None,
    Help { scroll: usize },
    Info,
}

/// The search prompt being typed.
#[derive(Clone, Debug, Default)]
pub struct Prompt {
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Search {
    pub query: String,
    /// All matches: (page, match), in document order.
    pub matches: Vec<(usize, Match)>,
    pub current: Option<usize>,
}

/// The lens: a magnified box over part of the page.
#[derive(Clone, Copy, Debug)]
pub struct Lens {
    /// Centre, in cells of the main view's page grid.
    pub col: i32,
    pub row: i32,
    pub magnification: f32,
}

/// One outline entry, flattened: nesting depth, title, destination
/// (page, y).
#[derive(Clone, Debug)]
pub struct OutlineEntry {
    pub depth: usize,
    pub title: String,
    pub dest: Option<(usize, Option<f32>)>,
}

/// The outline panel.
#[derive(Clone, Debug)]
pub struct OutlinePanel {
    /// (depth, title, page, y) of every item, flattened.
    pub items: Vec<OutlineEntry>,
    pub selected: usize,
    pub scroll: usize,
}

/// Reflow mode position: line `line` of page `page`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReflowPos {
    pub page: usize,
    pub line: usize,
}

pub struct App {
    pub session: Arc<Session>,
    pub settings: Settings,
    pub keymap: Keymap,
    pub tiles: Tiles,
    pub page: usize,
    pub mode: ViewMode,
    /// How the zoom was chosen, and the mapping it gave.
    pub zoom: ZoomSpec,
    pub mapping: Mapping,
    /// Top-left of the view, in cells of the page grid.
    pub scroll_col: i32,
    pub scroll_row: i32,
    pub view_cols: u16,
    pub view_rows: u16,
    pub reflow: ReflowPos,
    reflow_cache: HashMap<(usize, u16), Arc<Vec<RLine>>>,
    pub lens: Option<Lens>,
    pub search: Search,
    pub prompt: Option<Prompt>,
    pub outline: Option<OutlinePanel>,
    links: HashMap<usize, Arc<Vec<Link>>>,
    /// Index into [`App::visible_links`] of the focused link.
    pub link_focus: Option<usize>,
    pub overlay: Overlay,
    pub message: Option<(String, Instant)>,
    count: Option<usize>,
    pub quit: bool,
    pub dirty: bool,
    pub last_frame_bytes: usize,
    pub last_frame_ms: f64,
    /// OSC 52 sequences to send after the next frame.
    pub clipboard_out: Option<String>,
    pub reload_requested: bool,
}

impl App {
    pub fn new(
        session: Arc<Session>,
        settings: Settings,
        keymap: Keymap,
        tiles: Tiles,
        page: usize,
        cols: u16,
        rows: u16,
    ) -> App {
        let page = page.min(session.page_count().saturating_sub(1));
        let mut app = App {
            mode: settings.mode,
            zoom: settings.zoom,
            mapping: Mapping::new(12.0, settings.cell_aspect),
            session,
            settings,
            keymap,
            tiles,
            page,
            scroll_col: 0,
            scroll_row: 0,
            view_cols: cols,
            view_rows: rows.saturating_sub(1).max(1),
            reflow: ReflowPos { page, line: 0 },
            reflow_cache: HashMap::new(),
            lens: None,
            search: Search::default(),
            prompt: None,
            outline: None,
            links: HashMap::new(),
            link_focus: None,
            overlay: Overlay::None,
            message: None,
            count: None,
            quit: false,
            dirty: true,
            last_frame_bytes: 0,
            last_frame_ms: 0.0,
            clipboard_out: None,
            reload_requested: false,
        };
        app.apply_zoom(app.zoom, None);
        // Start at the top-left of the page (centred if narrower).
        app.scroll_row = 0;
        app.scroll_col = 0;
        app.clamp_scroll();
        if app.session.is_scanned(app.page) {
            app.say("scanned page without text: shown as an image");
        }
        app
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), Instant::now()));
        self.dirty = true;
    }

    // ---- geometry ---------------------------------------------------------

    pub fn page_cells(&self) -> (i32, i32) {
        let (c, r) = self.mapping.page_cells(self.session.page_size(self.page));
        (c as i32, r as i32)
    }

    pub fn view_key(&self) -> ViewKey {
        ViewKey::new(self.page, &self.mapping, self.mode)
    }

    /// The mapping for the current zoom spec on the current page.
    fn mapping_for(&self, spec: ZoomSpec, page: usize) -> Mapping {
        let mode = if self.mode == ViewMode::Raster {
            ViewMode::Raster
        } else {
            ViewMode::Layout
        };
        self.session.mapping(
            spec,
            mode,
            page,
            self.view_cols,
            self.view_rows,
            self.settings.cell_aspect,
        )
    }

    /// Change the zoom, keeping the page point at `anchor` (view cell) in place.
    fn apply_zoom(&mut self, spec: ZoomSpec, anchor: Option<(f32, f32)>) {
        let (ax, ay) = anchor.unwrap_or((self.view_cols as f32 / 2.0, self.view_rows as f32 / 2.0));
        let old = self.mapping;
        let (px, py) = old.to_page(self.scroll_col as f32 + ax, self.scroll_row as f32 + ay);
        self.zoom = spec;
        self.mapping = self.mapping_for(spec, self.page);
        let (c, r) = self.mapping.to_cell(px, py);
        self.scroll_col = (c - ax).round() as i32;
        self.scroll_row = (r - ay).round() as i32;
        self.clamp_scroll();
        self.dirty = true;
    }

    /// Zoom by a factor around the view centre.
    fn zoom_by(&mut self, factor: f32) {
        let (ax, ay) = (self.view_cols as f32 / 2.0, self.view_rows as f32 / 2.0);
        let (px, py) = self
            .mapping
            .to_page(self.scroll_col as f32 + ax, self.scroll_row as f32 + ay);
        let m = self.mapping.zoomed(factor);
        if (m.row_pitch - self.mapping.row_pitch).abs() < 1e-4 {
            return;
        }
        self.mapping = if self.mode == ViewMode::Raster {
            m.with_stretch(1.0)
        } else {
            m
        };
        self.zoom = ZoomSpec::Percent(self.mapping.zoom_percent());
        let (c, r) = self.mapping.to_cell(px, py);
        self.scroll_col = (c - ax).round() as i32;
        self.scroll_row = (r - ay).round() as i32;
        self.clamp_scroll();
        self.dirty = true;
    }

    pub fn clamp_scroll(&mut self) {
        let (pc, pr) = self.page_cells();
        let vc = self.view_cols as i32;
        let vr = self.view_rows as i32;
        self.scroll_col = if pc <= vc {
            -((vc - pc) / 2)
        } else {
            self.scroll_col.clamp(0, pc - vc)
        };
        self.scroll_row = if pr <= vr { 0 } else { self.scroll_row.clamp(0, pr - vr) };
    }

    pub fn window(&self) -> CellRect {
        CellRect::new(self.scroll_col, self.scroll_row, self.view_cols, self.view_rows)
    }

    /// Whether all text in view could be drawn as characters.
    pub fn legible(&self) -> bool {
        if self.mode != ViewMode::Layout {
            return true;
        }
        let layout = self.tiles.layout(self.view_key());
        layout.legible_in(self.scroll_row, self.scroll_row + self.view_rows as i32)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.view_cols = cols.max(1);
        self.view_rows = rows.saturating_sub(1).max(1);
        if matches!(self.zoom, ZoomSpec::Width | ZoomSpec::Page) {
            self.apply_zoom(self.zoom, None);
        }
        self.clamp_scroll();
        self.dirty = true;
    }

    // ---- pages ------------------------------------------------------------

    pub fn goto_page(&mut self, page: usize, y: Option<f32>) {
        let page = page.min(self.session.page_count().saturating_sub(1));
        if page != self.page {
            self.page = page;
            self.link_focus = None;
            // Same zoom; text zoom re-aligns rows to the new page's text.
            if self.zoom == ZoomSpec::Text && self.mode != ViewMode::Raster {
                self.mapping = self.session.with_phase(page, self.mapping);
            } else if matches!(self.zoom, ZoomSpec::Width | ZoomSpec::Page) {
                self.mapping = self.mapping_for(self.zoom, page);
            }
            if self.session.is_scanned(page) {
                self.say("scanned page without text: shown as an image");
            }
        }
        self.scroll_row = match y {
            Some(y) => self.mapping.row_of(y).floor() as i32 - 1,
            None => 0,
        };
        self.reflow = ReflowPos { page, line: 0 };
        if let Some(y) = y {
            let lines = self.reflow_lines(page);
            self.reflow.line = lines.iter().position(|l| l.y >= y - 1.0).unwrap_or(0);
        }
        self.clamp_scroll();
        self.dirty = true;
    }

    fn scroll_rows(&mut self, n: i32) {
        self.link_focus = None;
        if self.mode == ViewMode::Reflow {
            self.reflow_scroll(n);
            return;
        }
        let (_, pr) = self.page_cells();
        let vr = self.view_rows as i32;
        let max = (pr - vr).max(0);
        let target = self.scroll_row + n;
        if target > max && self.scroll_row >= max && n > 0 && self.page + 1 < self.session.page_count() {
            // Past the bottom: continue on the next page.
            let col = self.scroll_col;
            self.goto_page(self.page + 1, None);
            self.scroll_col = col;
            self.clamp_scroll();
        } else if target < 0 && self.scroll_row <= 0 && n < 0 && self.page > 0 {
            let col = self.scroll_col;
            self.goto_page(self.page - 1, None);
            let (_, pr) = self.page_cells();
            self.scroll_row = (pr - vr).max(0);
            self.scroll_col = col;
            self.clamp_scroll();
        } else {
            self.scroll_row = target;
            self.clamp_scroll();
        }
        self.dirty = true;
    }

    // ---- reflow -----------------------------------------------------------

    pub fn reflow_width(&self) -> u16 {
        self.view_cols.saturating_sub(2).clamp(20, 100)
    }

    pub fn reflow_lines(&mut self, page: usize) -> Arc<Vec<RLine>> {
        let w = self.reflow_width();
        if let Some(l) = self.reflow_cache.get(&(page, w)) {
            return l.clone();
        }
        let text = self.session.text(page);
        let g = self.session.graphics(page);
        let lines = Arc::new(sshpdf_layout::reflow::reflow(&text, page, w as usize, &g));
        self.reflow_cache.insert((page, w), lines.clone());
        lines
    }

    fn reflow_scroll(&mut self, n: i32) {
        let pages = self.session.page_count();
        let mut pos = self.reflow;
        if n > 0 {
            for _ in 0..n {
                // Each page shows its lines plus a separator line.
                let len = self.reflow_lines(pos.page).len() + 1;
                if pos.line + 1 < len {
                    pos.line += 1;
                } else if pos.page + 1 < pages {
                    pos = ReflowPos {
                        page: pos.page + 1,
                        line: 0,
                    };
                } else {
                    break;
                }
            }
        } else {
            for _ in 0..(-n) {
                if pos.line > 0 {
                    pos.line -= 1;
                } else if pos.page > 0 {
                    let len = self.reflow_lines(pos.page - 1).len() + 1;
                    pos = ReflowPos {
                        page: pos.page - 1,
                        line: len.saturating_sub(1),
                    };
                } else {
                    break;
                }
            }
        }
        self.reflow = pos;
        if pos.page != self.page {
            self.page = pos.page;
            if self.zoom == ZoomSpec::Text {
                self.mapping = self.session.with_phase(self.page, self.mapping);
            }
        }
        self.dirty = true;
    }

    fn toggle_mode(&mut self, target: ViewMode) {
        let new = if self.mode == target { ViewMode::Layout } else { target };
        // Keep the position: page and the y of the top of the view.
        let y = if self.mode == ViewMode::Reflow {
            let lines = self.reflow_lines(self.reflow.page);
            lines.get(self.reflow.line).map(|l| l.y)
        } else {
            Some(self.mapping.to_page(0.0, self.scroll_row as f32).1)
        };
        self.mode = new;
        self.lens = None;
        self.link_focus = None;
        let page = if self.mode == ViewMode::Reflow {
            self.page
        } else {
            self.reflow.page
        };
        self.page = page;
        self.mapping = self.mapping_for(self.zoom, page);
        if let Some(y) = y.filter(|y| y.is_finite()) {
            self.goto_page(page, Some(y));
        } else {
            self.goto_page(page, None);
        }
        self.say(format!("{} mode", new.name()));
    }

    // ---- tiles ------------------------------------------------------------

    /// Tiles to render, most urgent first: visible, rest of the page, the
    /// next page's top, the previous page's bottom.
    pub fn wanted_tiles(&self) -> Vec<TileKey> {
        if self.mode == ViewMode::Reflow {
            return Vec::new();
        }
        let mut out = Vec::new();
        let view = self.view_key();
        let (pc, pr) = self.page_cells();
        let tiles_x = (pc + TILE_COLS - 1) / TILE_COLS;
        let tiles_y = (pr + TILE_ROWS - 1) / TILE_ROWS;
        let w = self.window();
        let tx0 = (w.col.max(0) / TILE_COLS).min(tiles_x - 1).max(0);
        let tx1 = ((w.col + w.cols as i32 - 1).max(0) / TILE_COLS).min(tiles_x - 1);
        let ty0 = (w.row.max(0) / TILE_ROWS).min(tiles_y - 1).max(0);
        let ty1 = ((w.row + w.rows as i32 - 1).max(0) / TILE_ROWS).min(tiles_y - 1);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                out.push(TileKey { view, tx, ty });
            }
        }
        // The rest of this page, nearest rows first.
        let mut rest: Vec<(i32, TileKey)> = Vec::new();
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let k = TileKey { view, tx, ty };
                if !out.contains(&k) {
                    let d = if ty > ty1 { ty - ty1 } else { ty0 - ty };
                    rest.push((d, k));
                }
            }
        }
        rest.sort_by_key(|(d, _)| *d);
        out.extend(rest.into_iter().map(|(_, k)| k));
        // Neighbour pages at the same zoom.
        let n = self.session.page_count();
        for (p, top) in [(self.page + 1, true), (self.page.wrapping_sub(1), false)] {
            if p < n {
                let m = if self.zoom == ZoomSpec::Text && self.mode != ViewMode::Raster {
                    self.session.with_phase(p, self.mapping)
                } else {
                    self.mapping
                };
                let v = ViewKey::new(p, &m, self.mode);
                let (pc, pr) = m.page_cells(self.session.page_size(p));
                let tiles_x = (pc as i32 + TILE_COLS - 1) / TILE_COLS;
                let tiles_y = (pr as i32 + TILE_ROWS - 1) / TILE_ROWS;
                let ty = if top { 0 } else { tiles_y - 1 };
                for tx in 0..tiles_x {
                    out.push(TileKey { view: v, tx, ty });
                }
            }
        }
        out
    }

    // ---- links ------------------------------------------------------------

    pub fn page_links(&mut self, page: usize) -> Arc<Vec<Link>> {
        if let Some(l) = self.links.get(&page) {
            return l.clone();
        }
        let l = Arc::new(self.session.doc.links(page));
        self.links.insert(page, l.clone());
        l
    }

    /// Links on screen with their cell rectangles (view coordinates).
    pub fn visible_links(&mut self) -> Vec<(Link, CellRect)> {
        if self.mode == ViewMode::Reflow {
            return Vec::new();
        }
        let links = self.page_links(self.page);
        let w = self.window();
        let mut out = Vec::new();
        for l in links.iter() {
            let (c0, r0) = self.mapping.to_cell(l.rect.x0, l.rect.y0);
            let (c1, r1) = self.mapping.to_cell(l.rect.x1, l.rect.y1);
            let (c0, r0) = (c0.floor() as i32, r0.floor() as i32);
            let (c1, r1) = ((c1.ceil() as i32).max(c0 + 1), (r1.ceil() as i32).max(r0 + 1));
            if c1 <= w.col || c0 >= w.col + w.cols as i32 || r1 <= w.row || r0 >= w.row + w.rows as i32 {
                continue;
            }
            out.push((
                l.clone(),
                CellRect::new(c0 - w.col, r0 - w.row, (c1 - c0) as u16, (r1 - r0) as u16),
            ));
        }
        out.sort_by_key(|(_, r)| (r.row, r.col));
        out
    }

    fn follow(&mut self, target: LinkTarget) {
        match target {
            LinkTarget::Internal(d) => {
                self.goto_page(d.page, d.y);
                self.say(format!("page {}", d.page + 1));
            }
            LinkTarget::External(url) => {
                self.clipboard_out = Some(url.clone());
                self.say(format!("{url}  (copied)"));
            }
        }
    }

    // ---- search -----------------------------------------------------------

    fn run_search(&mut self, query: String) {
        let mut matches = Vec::new();
        for p in 0..self.session.page_count() {
            for m in sshpdf_layout::search::find(&self.session.text(p), &query) {
                matches.push((p, m));
            }
        }
        let n = matches.len();
        self.search = Search {
            query: query.clone(),
            matches,
            current: None,
        };
        if n == 0 {
            self.say(format!("no match for \"{query}\""));
        } else {
            self.next_match(true, true);
        }
    }

    fn next_match(&mut self, forward: bool, from_here: bool) {
        let n = self.search.matches.len();
        if n == 0 {
            if !self.search.query.is_empty() {
                self.say(format!("no match for \"{}\"", self.search.query));
            }
            return;
        }
        let next = match (self.search.current, from_here) {
            (Some(c), false) => {
                if forward {
                    (c + 1) % n
                } else {
                    (c + n - 1) % n
                }
            }
            _ => {
                let here = (self.page, self.mapping.to_page(0.0, self.scroll_row as f32).1);
                self.search
                    .matches
                    .iter()
                    .position(|(p, m)| (*p, m.bbox.y0) >= here)
                    .unwrap_or(0)
            }
        };
        self.search.current = Some(next);
        let (page, m) = self.search.matches[next].clone();
        if self.mode == ViewMode::Reflow {
            self.goto_page(page, Some(m.bbox.y0));
        } else {
            if page != self.page {
                self.goto_page(page, None);
            }
            let (_, r) = self.mapping.to_cell(0.0, m.bbox.y0);
            let (c, _) = self.mapping.to_cell(m.bbox.x0, 0.0);
            let vr = self.view_rows as i32;
            let r = r.floor() as i32;
            if r < self.scroll_row || r >= self.scroll_row + vr {
                self.scroll_row = r - vr / 3;
            }
            let vc = self.view_cols as i32;
            let c = c.floor() as i32;
            if c < self.scroll_col || c >= self.scroll_col + vc {
                self.scroll_col = c - vc / 4;
            }
            self.clamp_scroll();
        }
        self.say(format!("match {} of {} for \"{}\"", next + 1, n, self.search.query));
    }

    // ---- outline ----------------------------------------------------------

    fn open_outline(&mut self) {
        let outline = self.session.doc.outline();
        if outline.is_empty() {
            self.say("this document has no outline");
            return;
        }
        fn flat(items: &[OutlineItem], depth: usize, out: &mut Vec<OutlineEntry>) {
            for i in items {
                out.push(OutlineEntry {
                    depth,
                    title: i.title.clone(),
                    dest: i.dest.map(|d| (d.page, d.y)),
                });
                flat(&i.children, depth + 1, out);
            }
        }
        let mut items = Vec::new();
        flat(&outline, 0, &mut items);
        // Select the last entry at or before the current page.
        let selected = items
            .iter()
            .rposition(|e| e.dest.is_some_and(|(p, _)| p <= self.page))
            .unwrap_or(0);
        self.outline = Some(OutlinePanel {
            items,
            selected,
            scroll: selected.saturating_sub(5),
        });
        self.dirty = true;
    }

    fn outline_key(&mut self, key: &Key) {
        let Some(panel) = self.outline.as_mut() else {
            return;
        };
        let n = panel.items.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('o') | KeyCode::Char('q') => self.outline = None,
            KeyCode::Down | KeyCode::Char('j') => panel.selected = (panel.selected + 1).min(n - 1),
            KeyCode::Up | KeyCode::Char('k') => panel.selected = panel.selected.saturating_sub(1),
            KeyCode::PageDown | KeyCode::Char(' ') => panel.selected = (panel.selected + 10).min(n - 1),
            KeyCode::PageUp | KeyCode::Char('b') => panel.selected = panel.selected.saturating_sub(10),
            KeyCode::Home | KeyCode::Char('g') => panel.selected = 0,
            KeyCode::End | KeyCode::Char('G') => panel.selected = n - 1,
            KeyCode::Enter => {
                let dest = panel.items[panel.selected].dest;
                self.outline = None;
                match dest {
                    Some((p, y)) => self.goto_page(p, y),
                    None => self.say("this entry has no destination"),
                }
            }
            _ => {}
        }
        self.dirty = true;
    }

    // ---- lens -------------------------------------------------------------

    fn lens_move(&mut self, dc: i32, dr: i32) {
        if let Some(l) = self.lens.as_mut() {
            l.col += dc;
            l.row += dr;
            let (pc, pr) = {
                let (c, r) = self.mapping.page_cells(self.session.page_size(self.page));
                (c as i32, r as i32)
            };
            l.col = l.col.clamp(0, pc.max(1) - 1);
            l.row = l.row.clamp(0, pr.max(1) - 1);
            // Keep the lens centre on screen.
            let (vc, vr) = (self.view_cols as i32, self.view_rows as i32);
            let (lc, lr) = (l.col, l.row);
            if lr < self.scroll_row + 2 {
                self.scroll_row = lr - 2;
            } else if lr >= self.scroll_row + vr - 2 {
                self.scroll_row = lr - vr + 3;
            }
            if lc < self.scroll_col + 2 {
                self.scroll_col = lc - 2;
            } else if lc >= self.scroll_col + vc - 2 {
                self.scroll_col = lc - vc + 3;
            }
            self.clamp_scroll();
            self.dirty = true;
        }
    }

    // ---- input ------------------------------------------------------------

    pub fn on_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollDown => self.scroll_rows(3),
            MouseEventKind::ScrollUp => self.scroll_rows(-3),
            MouseEventKind::ScrollRight => {
                self.scroll_col += 4;
                self.clamp_scroll();
                self.dirty = true;
            }
            MouseEventKind::ScrollLeft => {
                self.scroll_col -= 4;
                self.clamp_scroll();
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn on_key(&mut self, e: KeyEvent) {
        if e.kind == KeyEventKind::Release {
            return;
        }
        let key = Key::from_event(&e);
        // The search prompt takes all keys.
        if let Some(p) = self.prompt.as_mut() {
            match key.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Enter => {
                    let q = std::mem::take(&mut p.text);
                    self.prompt = None;
                    if !q.trim().is_empty() {
                        self.run_search(q);
                    }
                }
                KeyCode::Backspace => {
                    p.text.pop();
                }
                KeyCode::Char(c) if !key.ctrl => p.text.push(c),
                _ => {}
            }
            self.dirty = true;
            return;
        }
        if self.outline.is_some() {
            self.outline_key(&key);
            return;
        }
        match self.overlay {
            Overlay::Help { scroll } => {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => self.overlay = Overlay::Help { scroll: scroll + 1 },
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.overlay = Overlay::Help {
                            scroll: scroll.saturating_sub(1),
                        }
                    }
                    _ => self.overlay = Overlay::None,
                }
                self.dirty = true;
                return;
            }
            Overlay::Info => {
                self.overlay = Overlay::None;
                self.dirty = true;
                return;
            }
            Overlay::None => {}
        }
        if key.code == KeyCode::Esc {
            self.count = None;
            if self.lens.is_some() {
                self.lens = None;
            } else if self.link_focus.is_some() {
                self.link_focus = None;
            } else {
                self.search.current = None;
                self.message = None;
            }
            self.dirty = true;
            return;
        }
        // A count before a command: "12G", "5j".
        if let KeyCode::Char(c @ '0'..='9') = key.code
            && !key.ctrl
            && !key.alt
            && (c != '0' || self.count.is_some())
        {
            let d = c as usize - '0' as usize;
            self.count = Some(
                self.count
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(d)
                    .min(1_000_000),
            );
            self.say(format!("{}", self.count.unwrap_or(0)));
            return;
        }
        let count = self.count.take();
        if count.is_some() {
            // The count shown while typing it.
            self.message = None;
            self.dirty = true;
        }
        let Some(action) = self.keymap.action(&key) else {
            return;
        };
        // The lens takes movement and magnification keys.
        if let Some(lens) = self.lens {
            let step = count.unwrap_or(1) as i32;
            match action {
                Action::ScrollDown => return self.lens_move(0, step),
                Action::ScrollUp => return self.lens_move(0, -step),
                Action::ScrollLeft => return self.lens_move(-step, 0),
                Action::ScrollRight => return self.lens_move(step, 0),
                Action::ZoomIn | Action::ZoomOut => {
                    let m = if action == Action::ZoomIn {
                        lens.magnification + 1.0
                    } else {
                        lens.magnification - 1.0
                    };
                    let m = m.clamp(2.0, 8.0);
                    self.lens = Some(Lens {
                        magnification: m,
                        ..lens
                    });
                    self.say(format!("lens {m:.0}×"));
                    return;
                }
                _ => {}
            }
        }
        self.act(action, count);
    }

    pub fn act(&mut self, action: Action, count: Option<usize>) {
        let n = count.unwrap_or(1) as i32;
        let screen = (self.view_rows as i32 - 2).max(1);
        match action {
            Action::ScrollDown => self.scroll_rows(n),
            Action::ScrollUp => self.scroll_rows(-n),
            Action::ScrollLeft => {
                self.scroll_col -= 4 * n;
                self.clamp_scroll();
                self.dirty = true;
            }
            Action::ScrollRight => {
                self.scroll_col += 4 * n;
                self.clamp_scroll();
                self.dirty = true;
            }
            Action::ScreenDown => self.scroll_rows(screen * n),
            Action::ScreenUp => self.scroll_rows(-screen * n),
            Action::HalfDown => self.scroll_rows(screen / 2 * n),
            Action::HalfUp => self.scroll_rows(-(screen / 2) * n),
            Action::NextPage => {
                let p = (self.page + n as usize).min(self.session.page_count() - 1);
                self.goto_page(p, None);
            }
            Action::PrevPage => {
                let p = self.page.saturating_sub(n as usize);
                self.goto_page(p, None);
            }
            Action::FirstPage => self.goto_page(0, None),
            Action::LastPage => match count {
                Some(c) => {
                    if c == 0 || c > self.session.page_count() {
                        self.say(format!("no page {c} (1-{})", self.session.page_count()));
                    } else {
                        self.goto_page(c - 1, None);
                    }
                }
                None => {
                    let last = self.session.page_count() - 1;
                    self.goto_page(last, None);
                }
            },
            Action::ZoomIn => self.zoom_by(1.25f32.powi(n)),
            Action::ZoomOut => self.zoom_by(1.0 / 1.25f32.powi(n)),
            Action::ZoomText => self.apply_zoom(ZoomSpec::Text, None),
            Action::FitWidth => self.apply_zoom(ZoomSpec::Width, None),
            Action::FitPage => self.apply_zoom(ZoomSpec::Page, None),
            Action::Lens => {
                if self.mode == ViewMode::Reflow {
                    self.say("the lens works in layout and raster mode");
                } else if self.lens.is_some() {
                    self.lens = None;
                    self.dirty = true;
                } else {
                    self.lens = Some(Lens {
                        col: self.scroll_col + self.view_cols as i32 / 2,
                        row: self.scroll_row + self.view_rows as i32 / 2,
                        magnification: self.settings.lens_magnification,
                    });
                    self.lens_move(0, 0);
                    self.say("lens: arrows/hjkl move, +/- magnify, z or Esc closes");
                }
            }
            Action::Search => {
                self.prompt = Some(Prompt::default());
                self.dirty = true;
            }
            Action::SearchNext => self.next_match(true, false),
            Action::SearchPrev => self.next_match(false, false),
            Action::Outline => self.open_outline(),
            Action::NextLink | Action::PrevLink => {
                if self.mode == ViewMode::Reflow {
                    self.say("press Enter on a figure line to view it");
                    return;
                }
                let n = self.visible_links().len();
                if n == 0 {
                    self.say("no links on screen");
                    return;
                }
                let f = match (self.link_focus, action == Action::NextLink) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(f), true) => (f + 1) % n,
                    (Some(f), false) => (f + n - 1) % n,
                };
                self.link_focus = Some(f);
                let links = self.visible_links();
                let target = match &links[f].0.target {
                    LinkTarget::Internal(d) => format!("→ page {}", d.page + 1),
                    LinkTarget::External(u) => format!("→ {u}"),
                };
                self.say(format!("link {} of {}  {target}  (Enter follows)", f + 1, n));
            }
            Action::FollowLink => {
                if self.mode == ViewMode::Reflow {
                    self.open_figure();
                    return;
                }
                let links = self.visible_links();
                match self.link_focus.and_then(|f| links.get(f)) {
                    Some((l, _)) => {
                        let t = l.target.clone();
                        self.link_focus = None;
                        self.follow(t);
                    }
                    None => self.say("Tab selects a link first"),
                }
            }
            Action::Copy => {
                let text = self.page_text(self.page);
                let chars = text.chars().count();
                self.clipboard_out = Some(text);
                self.say(format!(
                    "copied the text of page {} ({chars} characters)",
                    self.page + 1
                ));
            }
            Action::Info => {
                self.overlay = Overlay::Info;
                self.dirty = true;
            }
            Action::Help => {
                self.overlay = Overlay::Help { scroll: 0 };
                self.dirty = true;
            }
            Action::Reflow => self.toggle_mode(ViewMode::Reflow),
            Action::Raster => self.toggle_mode(ViewMode::Raster),
            Action::Reload => {
                self.reload_requested = true;
            }
            Action::Quit => self.quit = true,
        }
    }

    /// The reflowed text of a page, as plain lines.
    pub fn page_text(&mut self, page: usize) -> String {
        let text = self.session.text(page);
        let g = self.session.graphics(page);
        sshpdf_layout::reflow::reflow(&text, page, 80, &g)
            .iter()
            .map(|l| l.plain())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// In reflow mode: show the first figure on screen in raster mode.
    fn open_figure(&mut self) {
        let mut pos = self.reflow;
        for _ in 0..self.view_rows {
            let lines = self.reflow_lines(pos.page);
            if let Some(l) = lines.get(pos.line)
                && let LineKind::Figure { index } = l.kind
            {
                let g = self.session.graphics(pos.page);
                let size = self.session.page_size(pos.page);
                let mut figs = sshpdf_layout::reflow::figures(&g, size.w * size.h);
                figs.sort_by(|a, b| a.rect.y0.total_cmp(&b.rect.y0).then(a.rect.x0.total_cmp(&b.rect.x0)));
                let y = figs.get(index).map(|f| f.rect.y0);
                self.mode = ViewMode::Raster;
                self.page = pos.page;
                self.mapping = self.mapping_for(self.zoom, pos.page);
                self.scroll_row = y.map_or(0, |y| self.mapping.row_of(y).floor() as i32 - 1);
                self.clamp_scroll();
                self.say("figure in raster mode (R goes back)");
                return;
            }
            if pos.line + 1 < lines.len() {
                pos.line += 1;
            } else if pos.page + 1 < self.session.page_count() {
                pos = ReflowPos {
                    page: pos.page + 1,
                    line: 0,
                };
            } else {
                break;
            }
        }
        self.say("no figure on screen");
    }

    /// Show a new version of the document (file changed on disk).
    pub fn replace_session(&mut self, session: Arc<Session>) {
        self.tiles.reset(Some(session.clone()), None);
        self.session = session;
        self.links.clear();
        self.reflow_cache.clear();
        self.search.matches.clear();
        self.search.current = None;
        let page = self.page.min(self.session.page_count().saturating_sub(1));
        self.page = page;
        self.mapping = self.mapping_for(self.zoom, page);
        self.clamp_scroll();
        self.say("reloaded");
    }
}
