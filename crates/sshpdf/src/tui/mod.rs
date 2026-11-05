//! The interactive viewer.

mod app;
mod draw;
pub mod keys;
mod tiles;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::Engine;
use sshpdf_doc::OpenError;
use sshpdf_term::backend::TermBackend;
use ratatui::Terminal;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers};
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};

use crate::cli::Cli;
use crate::config::Settings;
use crate::session::Session;
use crate::state::{self, State};

use app::App;
use keys::Keymap;
use tiles::Tiles;

/// Messages for the main loop.
pub enum Msg {
    Input(Event),
    TileReady,
    FileChanged,
}

/// Shortest time between two frames (merges held-down keys).
const FRAME_TIME: Duration = Duration::from_millis(16);
/// Quiet time after a file change before reloading.
const RELOAD_DELAY: Duration = Duration::from_millis(300);

/// Ask for a password on the terminal, without echo.
fn read_password(prompt: &str) -> Result<String> {
    let mut out = io::stderr();
    write!(out, "{prompt}")?;
    out.flush()?;
    terminal::enable_raw_mode()?;
    let mut pw = String::new();
    let result = loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind != event::KeyEventKind::Release => match k.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Esc => break Err(anyhow::anyhow!("cancelled")),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err(anyhow::anyhow!("cancelled"));
                }
                KeyCode::Backspace => {
                    pw.pop();
                }
                KeyCode::Char(c) => pw.push(c),
                _ => {}
            },
            Ok(_) => {}
            Err(e) => break Err(e.into()),
        }
    };
    terminal::disable_raw_mode()?;
    writeln!(out)?;
    result.map(|()| pw)
}

/// Open the file, asking for a password if it needs one.
fn open_session(path: &Path, password: Option<String>) -> Result<(Session, Option<String>)> {
    match Session::open(path, password.as_deref()) {
        Ok(s) => return Ok((s, password)),
        Err(OpenError::PasswordRequired) => {}
        Err(OpenError::WrongPassword) if password.is_some() => {
            eprintln!("sshpdf: wrong password for {}", path.display());
        }
        Err(e) => bail!("{}: {e}", path.display()),
    }
    for _ in 0..3 {
        let pw = read_password(&format!("Password for {}: ", path.display()))?;
        match Session::open(path, Some(&pw)) {
            Ok(s) => return Ok((s, Some(pw))),
            Err(OpenError::WrongPassword) => eprintln!("wrong password"),
            Err(e) => bail!("{}: {e}", path.display()),
        }
    }
    bail!("no valid password for {}", path.display())
}

/// Copy text to the clipboard with OSC 52 (works over SSH; tmux needs
/// `set -g set-clipboard on`).
fn osc52(text: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;c;{b64}\x07")
}

/// Watch the file for changes (editors often replace it, so watch its folder).
fn watch(path: &Path, tx: mpsc::Sender<Msg>) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let target = std::fs::canonicalize(path).ok()?;
    let name = target.file_name()?.to_os_string();
    let dir = target.parent()?.to_path_buf();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res
            && !ev.kind.is_access()
            && ev.paths.iter().any(|p| p.file_name() == Some(name.as_os_str()))
        {
            let _ = tx.send(Msg::FileChanged);
        }
    })
    .ok()?;
    w.watch(&dir, RecursiveMode::NonRecursive).ok()?;
    Some(w)
}

fn restore_terminal() {
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen, cursor::Show);
    let _ = terminal::disable_raw_mode();
}

pub fn run(cli: &Cli, settings: Settings) -> Result<()> {
    let path: PathBuf = cli.file.clone().context("no FILE given (see --help)")?;
    let keymap = Keymap::new(&settings.keys)?;
    let (session, password) = open_session(&path, cli.password.clone())?;
    let session = Arc::new(session);
    let state_path = state::default_path();
    let mut state = state_path.as_deref().map(State::load).unwrap_or_default();
    let start_page = match cli.page {
        Some(p) => p.saturating_sub(1),
        None => state.last_page(&path).unwrap_or(0),
    };

    let (tx, rx) = mpsc::channel::<Msg>();
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(e) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture, cursor::Hide) {
        restore_terminal();
        return Err(e.into());
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    let result = (|| -> Result<()> {
        let backend = TermBackend::new(io::stdout(), settings.caps.depth, settings.caps.robust);
        let mut term = Terminal::new(backend)?;
        term.clear()?;
        {
            let tx = tx.clone();
            std::thread::Builder::new().name("input".into()).spawn(move || {
                while let Ok(ev) = event::read() {
                    if tx.send(Msg::Input(ev)).is_err() {
                        break;
                    }
                }
            })?;
        }
        let _watcher = watch(&path, tx.clone());
        let tiles = Tiles::new(session.clone(), settings.render_options(), tx.clone());
        let size = term.size()?;
        let mut app = App::new(
            session,
            settings.clone(),
            keymap,
            tiles,
            start_page,
            size.width,
            size.height,
        );
        let mut requested = Vec::new();
        let mut last_draw = Instant::now() - FRAME_TIME;
        let mut missing = 0;
        let mut reload_at: Option<Instant> = None;
        let started = Instant::now();
        let mut first_frame_logged = false;

        loop {
            if app.dirty && last_draw.elapsed() >= FRAME_TIME {
                let want = app.wanted_tiles();
                if want != requested {
                    app.tiles.request(&want);
                    requested = want;
                }
                let t0 = Instant::now();
                term.draw(|f| missing = draw::draw(f, &mut app))?;
                app.last_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
                app.last_frame_bytes = term.backend().last_frame_bytes();
                if let Some(text) = app.clipboard_out.take() {
                    let w = term.backend_mut().writer_mut();
                    w.write_all(osc52(&text).as_bytes())?;
                    w.flush()?;
                }
                app.dirty = false;
                last_draw = Instant::now();
                if !first_frame_logged && missing == 0 {
                    first_frame_logged = true;
                    log::info!(
                        "first complete frame after {:.0} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
            }
            if app.quit {
                break;
            }
            // Wait for something to happen.
            let mut timeout = Duration::from_secs(1);
            if app.dirty {
                timeout = FRAME_TIME.saturating_sub(last_draw.elapsed());
            }
            if let Some((_, at)) = &app.message {
                let left = draw::MESSAGE_TIME.saturating_sub(at.elapsed());
                if left.is_zero() {
                    app.message = None;
                    app.dirty = true;
                } else {
                    timeout = timeout.min(left);
                }
            }
            if let Some(at) = reload_at {
                timeout = timeout.min(at.saturating_duration_since(Instant::now()));
            }
            let first = match rx.recv_timeout(timeout) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            // Handle everything that is waiting before drawing again.
            let mut msgs: Vec<Msg> = first.into_iter().collect();
            msgs.extend(rx.try_iter());
            for msg in msgs {
                match msg {
                    Msg::Input(Event::Key(k)) => app.on_key(k),
                    Msg::Input(Event::Mouse(m)) => app.on_mouse(m),
                    Msg::Input(Event::Resize(w, h)) => {
                        term.autoresize()?;
                        app.resize(w, h);
                    }
                    Msg::Input(_) => {}
                    Msg::TileReady => {
                        if missing > 0 {
                            app.dirty = true;
                        }
                    }
                    Msg::FileChanged => reload_at = Some(Instant::now() + RELOAD_DELAY),
                }
            }
            if app.reload_requested {
                app.reload_requested = false;
                reload_at = Some(Instant::now());
            }
            if let Some(at) = reload_at
                && Instant::now() >= at
            {
                reload_at = None;
                match Session::open(&path, password.as_deref()) {
                    Ok(s) => app.replace_session(Arc::new(s)),
                    // Probably still being written: try again shortly.
                    Err(e) => {
                        log::debug!("reload failed: {e}");
                        app.say(format!("reload failed: {e}"));
                    }
                }
            }
        }
        if let Some(p) = &state_path {
            state.set_last_page(&path, app.page);
            if let Err(e) = state.save(p) {
                log::warn!("cannot save state: {e}");
            }
        }
        Ok(())
    })();
    restore_terminal();
    result
}
