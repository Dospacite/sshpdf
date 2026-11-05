//! End-to-end tests in tmux: start the viewer in a detached session, send
//! keys, capture the pane (with escapes) and compare with snapshots. This
//! covers key handling and the robust output mode (octant glyphs inside
//! tmux).
//!
//! Skipped when tmux isn't found. `SSHPDF_TMUX` can point at a tmux binary.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn tmux_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SSHPDF_TMUX") {
        return Some(PathBuf::from(p));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("tmux"))
        .find(|p| p.is_file())
}

struct Session {
    tmux: PathBuf,
    socket: PathBuf,
}

impl Session {
    fn start(tmux: PathBuf, cols: u16, rows: u16, args: &[&str]) -> Session {
        // Unix sockets need a short path; one server per test.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let socket = std::env::temp_dir().join(format!("sshpdf-e2e-{}-{n}.sock", std::process::id()));
        let mut cmd = vec![env!("CARGO_BIN_EXE_sshpdf").to_string()];
        cmd.extend(["--config".into(), "/dev/null".into(), "--theme".into(), "dark".into()]);
        cmd.extend(args.iter().map(|s| s.to_string()));
        // Keep the pane open after sshpdf exits, to see errors.
        let shell = format!(
            "{}; sleep 30",
            cmd.iter().map(|a| format!("'{a}'")).collect::<Vec<_>>().join(" ")
        );
        let s = Session { tmux, socket };
        let status = s
            .cmd(&[
                "new-session",
                "-d",
                "-s",
                "t",
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
                "env",
                "COLORTERM=truecolor",
                "sh",
                "-c",
                &shell,
            ])
            .status()
            .expect("start tmux");
        assert!(status.success());
        s
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.tmux);
        c.arg("-S").arg(&self.socket).arg("-f").arg("/dev/null").args(args);
        c
    }

    fn keys(&self, keys: &[&str]) {
        let mut args = vec!["send-keys", "-t", "t"];
        args.extend(keys);
        assert!(self.cmd(&args).status().unwrap().success());
    }

    fn capture(&self, escapes: bool) -> String {
        let mut args = vec!["capture-pane", "-p", "-t", "t"];
        if escapes {
            args.push("-e");
        }
        let out = self.cmd(&args).output().expect("capture-pane");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Wait until the status line (last row) contains `text` and the page
    /// is fully rendered.
    fn wait_for(&self, text: &str) -> String {
        let start = Instant::now();
        loop {
            let screen = self.capture(false);
            let status = screen.lines().last().unwrap_or("");
            if status.contains(text) && !status.contains("rendering") {
                // One more frame for the last tiles to land.
                std::thread::sleep(Duration::from_millis(150));
                return self.capture(false);
            }
            if start.elapsed() > Duration::from_secs(20) {
                panic!("timed out waiting for {text:?}; screen:\n{screen}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.cmd(&["kill-server"]).status();
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn corpus(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn navigate_paper_in_tmux() {
    let Some(tmux) = tmux_bin() else {
        eprintln!("tmux not found; skipping");
        return;
    };
    let paper = corpus("paper-2col.pdf");
    let s = Session::start(
        tmux,
        120,
        40,
        &[&paper, "--tier", "octant", "--colors", "true", "--page", "1"],
    );
    let screen = s.wait_for("1/9");
    assert!(screen.contains("INTRODUCTION"), "{screen}");
    if cfg!(target_arch = "x86_64") {
        insta::assert_snapshot!("tmux_paper_page1", s.capture(true).replace('\x1b', "\\e"));
    }
    // Next page, a page by number, first page.
    s.keys(&["J"]);
    s.wait_for("2/9");
    s.keys(&["5", "G"]);
    s.wait_for("5/9");
    s.keys(&["g"]);
    s.wait_for("1/9");
    // Reflow and back.
    s.keys(&["r"]);
    let screen = s.wait_for("reflow");
    assert!(screen.contains("Semiconductor qubit devices suffer"), "{screen}");
    s.keys(&["r"]);
    s.wait_for("layout");
    // Raster mode.
    s.keys(&["R"]);
    s.wait_for("raster");
    // Zoom in and back to text zoom.
    s.keys(&["R", "+"]);
    let screen = s.wait_for("layout");
    assert!(!screen.lines().last().unwrap().contains("text "), "{screen}");
    s.keys(&["="]);
    s.wait_for("text ");
    // Quit: the pane is left with the shell's sleep, no sshpdf status line.
    s.keys(&["q"]);
    let start = Instant::now();
    while s.capture(false).contains("1/9") {
        assert!(start.elapsed() < Duration::from_secs(5), "sshpdf did not quit");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn rows_stay_aligned_with_octants_in_tmux() {
    // Robust mode: every row of the captured screen is exactly as wide as
    // the pane, even with Unicode 16 octants that tmux may not know.
    let Some(tmux) = tmux_bin() else {
        eprintln!("tmux not found; skipping");
        return;
    };
    let brochure = corpus("brochure-photos.pdf");
    let s = Session::start(
        tmux,
        100,
        30,
        &[&brochure, "--tier", "octant", "--colors", "true", "--mode", "raster"],
    );
    let screen = s.wait_for("1/3");
    use unicode_width::UnicodeWidthStr;
    for (i, line) in screen.lines().enumerate().take(29) {
        assert!(line.width() <= 100, "row {i} is {} wide:\n{line}", line.width());
    }
    // The status line starts at column 0 (nothing pushed it).
    assert!(
        screen.lines().last().unwrap().starts_with(" brochure-photos.pdf"),
        "{screen}"
    );
}
