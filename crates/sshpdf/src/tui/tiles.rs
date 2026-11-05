//! Rendering page tiles on background threads, with a cache.
//!
//! A page at one zoom is split into tiles of [`TILE_ROWS`] rows and up to
//! [`TILE_COLS`] columns. The UI asks for the tiles it needs; workers render
//! them (visible ones first, then prefetch) and the UI draws whatever is
//! ready. Asking for a new set of tiles drops queued jobs that weren't
//! started, so a user paging quickly doesn't wait for stale work.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Instant;

use sshpdf_layout::{CellRect, Mapping, PageLayout, RenderOptions, render_cells};
use sshpdf_term::CellGrid;

use super::Msg;
use crate::config::ViewMode;
use crate::session::Session;

pub const TILE_ROWS: i32 = 32;
pub const TILE_COLS: i32 = 256;
/// Tiles kept in memory (about 250 KB each at most).
const CACHE_TILES: usize = 400;

/// A page at one zoom and mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewKey {
    pub page: usize,
    mapping: [u32; 4],
    raster: bool,
}

impl ViewKey {
    pub fn new(page: usize, m: &Mapping, mode: ViewMode) -> ViewKey {
        ViewKey {
            page,
            mapping: [
                m.row_pitch.to_bits(),
                m.cell_aspect.to_bits(),
                m.x_stretch.to_bits(),
                m.y_offset.to_bits(),
            ],
            raster: mode == ViewMode::Raster,
        }
    }

    pub fn mapping(&self) -> Mapping {
        Mapping {
            row_pitch: f32::from_bits(self.mapping[0]),
            cell_aspect: f32::from_bits(self.mapping[1]),
            x_stretch: f32::from_bits(self.mapping[2]),
            y_offset: f32::from_bits(self.mapping[3]),
        }
    }

    pub fn mode(&self) -> ViewMode {
        if self.raster {
            ViewMode::Raster
        } else {
            ViewMode::Layout
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub view: ViewKey,
    pub tx: i32,
    pub ty: i32,
}

impl TileKey {
    /// The cells this tile covers, clipped to the page grid.
    pub fn rect(&self, page_cols: i32, page_rows: i32) -> CellRect {
        let col = self.tx * TILE_COLS;
        let row = self.ty * TILE_ROWS;
        let cols = TILE_COLS.min(page_cols - col).max(0);
        let rows = TILE_ROWS.min(page_rows - row).max(0);
        CellRect::new(col, row, cols as u16, rows as u16)
    }
}

struct Job {
    key: TileKey,
    /// Lower is more urgent.
    priority: u32,
}

#[derive(Default)]
struct Stats {
    tiles_rendered: u64,
    render_ms_total: f64,
    last_render_ms: f64,
}

struct Shared {
    session: Mutex<Arc<Session>>,
    opts: Mutex<RenderOptions>,
    layouts: Mutex<HashMap<ViewKey, Arc<OnceLock<Arc<PageLayout>>>>>,
    tiles: Mutex<HashMap<TileKey, (Arc<CellGrid>, u64)>>,
    queue: Mutex<VecDeque<Job>>,
    cvar: Condvar,
    /// Access counter for the LRU.
    clock: AtomicU64,
    stop: AtomicBool,
    stats: Mutex<Stats>,
    tx: Sender<Msg>,
}

/// The tile renderer: a cache plus worker threads.
pub struct Tiles {
    shared: Arc<Shared>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl Shared {
    fn layout(&self, view: ViewKey) -> Arc<PageLayout> {
        let cell = {
            let mut map = self.layouts.lock().expect("layout lock");
            map.entry(view).or_insert_with(|| Arc::new(OnceLock::new())).clone()
        };
        cell.get_or_init(|| {
            let session = self.session.lock().expect("session lock").clone();
            Arc::new(session.layout(view.page, view.mapping(), view.mode()))
        })
        .clone()
    }

    fn render(&self, key: TileKey) {
        if self.tiles.lock().expect("tiles lock").contains_key(&key) {
            return;
        }
        let start = Instant::now();
        let layout = self.layout(key.view);
        let (pc, pr) = layout.grid_size();
        let rect = key.rect(pc as i32, pr as i32);
        let session = self.session.lock().expect("session lock").clone();
        let opts = *self.opts.lock().expect("opts lock");
        let grid = render_cells(&*session.doc, &layout, rect, &opts);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        {
            let mut s = self.stats.lock().expect("stats lock");
            s.tiles_rendered += 1;
            s.render_ms_total += ms;
            s.last_render_ms = ms;
        }
        log::debug!("tile {key:?} rendered in {ms:.1} ms");
        let stamp = self.clock.fetch_add(1, Ordering::Relaxed);
        let mut tiles = self.tiles.lock().expect("tiles lock");
        tiles.insert(key, (Arc::new(grid), stamp));
        if tiles.len() > CACHE_TILES {
            // Drop the least recently used quarter.
            let mut stamps: Vec<u64> = tiles.values().map(|(_, s)| *s).collect();
            stamps.sort_unstable();
            let cut = stamps[stamps.len() / 4];
            tiles.retain(|_, (_, s)| *s > cut);
        }
        drop(tiles);
        let _ = self.tx.send(Msg::TileReady);
    }

    fn worker(&self) {
        loop {
            let job = {
                let mut q = self.queue.lock().expect("queue lock");
                loop {
                    if self.stop.load(Ordering::Relaxed) {
                        return;
                    }
                    // Most urgent first.
                    if let Some(i) = q.iter().enumerate().min_by_key(|(_, j)| j.priority).map(|(i, _)| i) {
                        break q.remove(i).expect("index valid");
                    }
                    q = self.cvar.wait(q).expect("queue lock");
                }
            };
            self.render(job.key);
        }
    }
}

impl Tiles {
    pub fn new(session: Arc<Session>, opts: RenderOptions, tx: Sender<Msg>) -> Tiles {
        let shared = Arc::new(Shared {
            session: Mutex::new(session),
            opts: Mutex::new(opts),
            layouts: Mutex::new(HashMap::new()),
            tiles: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            cvar: Condvar::new(),
            clock: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            stats: Mutex::new(Stats::default()),
            tx,
        });
        // Each tile already uses all cores in the optimizer; two workers keep
        // one tile rendering while the other interprets the page.
        let n = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 3));
        let workers = (0..n)
            .map(|i| {
                let s = shared.clone();
                std::thread::Builder::new()
                    .name(format!("render-{i}"))
                    .spawn(move || s.worker())
                    .expect("spawn render thread")
            })
            .collect();
        Tiles { shared, workers }
    }

    /// The layout of a page at a zoom (built now if needed).
    pub fn layout(&self, view: ViewKey) -> Arc<PageLayout> {
        self.shared.layout(view)
    }

    /// A rendered tile, if ready.
    pub fn get(&self, key: &TileKey) -> Option<Arc<CellGrid>> {
        let mut tiles = self.shared.tiles.lock().expect("tiles lock");
        let stamp = self.shared.clock.fetch_add(1, Ordering::Relaxed);
        tiles.get_mut(key).map(|(g, s)| {
            *s = stamp;
            g.clone()
        })
    }

    /// Replace the queue: render `keys` (in order of urgency), skipping
    /// cached ones; queued jobs from before are dropped.
    pub fn request(&self, keys: &[TileKey]) {
        let tiles = self.shared.tiles.lock().expect("tiles lock");
        let jobs: VecDeque<Job> = keys
            .iter()
            .enumerate()
            .filter(|(_, k)| !tiles.contains_key(k))
            .map(|(i, k)| Job {
                key: *k,
                priority: i as u32,
            })
            .collect();
        drop(tiles);
        let mut q = self.shared.queue.lock().expect("queue lock");
        *q = jobs;
        drop(q);
        self.shared.cvar.notify_all();
    }

    /// Forget everything (new options, new file).
    pub fn reset(&self, session: Option<Arc<Session>>, opts: Option<RenderOptions>) {
        self.shared.queue.lock().expect("queue lock").clear();
        if let Some(s) = session {
            *self.shared.session.lock().expect("session lock") = s;
        }
        if let Some(o) = opts {
            *self.shared.opts.lock().expect("opts lock") = o;
        }
        self.shared.layouts.lock().expect("layout lock").clear();
        self.shared.tiles.lock().expect("tiles lock").clear();
    }

    pub fn pending(&self) -> usize {
        self.shared.queue.lock().expect("queue lock").len()
    }

    /// (tiles rendered, average ms, last ms, cached tiles)
    pub fn stats(&self) -> (u64, f64, f64, usize) {
        let s = self.shared.stats.lock().expect("stats lock");
        let avg = if s.tiles_rendered > 0 {
            s.render_ms_total / s.tiles_rendered as f64
        } else {
            0.0
        };
        let cached = self.shared.tiles.lock().expect("tiles lock").len();
        (s.tiles_rendered, avg, s.last_render_ms, cached)
    }
}

impl Drop for Tiles {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.cvar.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}
