//! Snapshot tests of `--dump` (and `--text`) on corpus pages.
//!
//! Every setting is given on the command line so the output doesn't depend
//! on the terminal or a config file. Escapes are kept, written as `\e`.
//! Update with `INSTA_UPDATE=always cargo test -p sshpdf --test snapshots`
//! (or `cargo insta review`), then look at the changes, ideally next to
//! `--dump-png` renders of the same pages.

use std::path::PathBuf;
use std::process::Command;

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(name)
}

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_sshpdf"))
        .args(["--config", "/dev/null", "--theme", "dark", "--cell-aspect", "0.5"])
        .args(args)
        .env_remove("COLORTERM")
        .env_remove("TMUX")
        .output()
        .expect("run sshpdf");
    assert!(
        out.status.success(),
        "sshpdf {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("utf-8 output")
        .replace('\x1b', "\\e")
}

fn dump(file: &str, page: u32, size: &str, tier: &str, colors: &str, mode: &str) -> String {
    let path = corpus(file);
    let page = page.to_string();
    run(&[
        "--dump",
        path.to_str().expect("utf-8 path"),
        "--page",
        &page,
        "--size",
        size,
        "--tier",
        tier,
        "--colors",
        colors,
        "--mode",
        mode,
    ])
}

/// Rendering (anti-aliasing) is floating point and can differ in the last
/// bit between CPU architectures; the raster snapshots were made on x86_64.
fn raster_snapshots() -> bool {
    cfg!(target_arch = "x86_64")
}

macro_rules! dump_snapshot {
    ($name:ident, $file:expr, $page:expr, $size:expr, $tier:expr, $colors:expr, $mode:expr) => {
        #[test]
        fn $name() {
            if !raster_snapshots() {
                return;
            }
            insta::assert_snapshot!(
                stringify!($name),
                dump($file, $page, $size, $tier, $colors, $mode)
            );
        }
    };
}

dump_snapshot!(
    paper_layout_octant,
    "paper-2col.pdf",
    1,
    "120x40",
    "octant",
    "true",
    "layout"
);
dump_snapshot!(
    manual_tables_quadrant_256,
    "manual-tables.pdf",
    2,
    "100x30",
    "quadrant",
    "256",
    "layout"
);
dump_snapshot!(
    synthetic_layout_octant,
    "synthetic.pdf",
    1,
    "120x45",
    "octant",
    "true",
    "layout"
);
dump_snapshot!(
    synthetic_raster_ascii_16,
    "synthetic.pdf",
    1,
    "80x30",
    "ascii",
    "16",
    "raster"
);
dump_snapshot!(
    brochure_raster_half_16,
    "brochure-photos.pdf",
    1,
    "60x30",
    "half",
    "16",
    "raster"
);
dump_snapshot!(
    brochure_raster_sextant,
    "brochure-photos.pdf",
    1,
    "60x30",
    "sextant",
    "true",
    "raster"
);
dump_snapshot!(
    scanned_raster_octant_256,
    "scanned.pdf",
    1,
    "60x30",
    "octant",
    "256",
    "raster"
);
dump_snapshot!(
    slides_layout_braille,
    "slides.pdf",
    2,
    "80x24",
    "braille",
    "true",
    "layout"
);
dump_snapshot!(cjk_layout_octant, "cjk.pdf", 1, "100x30", "octant", "true", "layout");
dump_snapshot!(
    rotated_page_layout,
    "rotated.pdf",
    2,
    "100x30",
    "octant",
    "true",
    "layout"
);

#[test]
fn paper_reflow_text() {
    let path = corpus("paper-2col.pdf");
    insta::assert_snapshot!(
        "paper_reflow_text",
        run(&["--text", path.to_str().unwrap(), "--page", "1"])
    );
}

#[test]
fn cjk_reflow_text() {
    let path = corpus("cjk.pdf");
    insta::assert_snapshot!(
        "cjk_reflow_text",
        run(&["--text", path.to_str().unwrap(), "--page", "1"])
    );
}

#[test]
fn every_tier_and_depth_dumps() {
    // Not a snapshot: every combination runs and fills the requested size.
    for tier in ["ascii", "half", "quadrant", "sextant", "octant", "braille"] {
        for colors in ["true", "256", "16"] {
            let out = dump("synthetic.pdf", 2, "50x12", tier, colors, "raster");
            assert_eq!(out.lines().count(), 12, "{tier} {colors}");
        }
    }
}

#[test]
fn encrypted_needs_password() {
    let path = corpus("encrypted.pdf");
    let out = Command::new(env!("CARGO_BIN_EXE_sshpdf"))
        .args(["--config", "/dev/null", "--text", path.to_str().unwrap(), "--page", "1"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("password"));
    let text = run(&["--text", path.to_str().unwrap(), "--page", "1", "--password", "sshpdf"]);
    assert!(text.len() > 100);
    // Owner-password-only files open without asking.
    let path = corpus("encrypted-owner-only.pdf");
    assert!(run(&["--text", path.to_str().unwrap(), "--page", "1"]).len() > 100);
}
