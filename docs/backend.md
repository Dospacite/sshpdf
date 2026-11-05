# PDF backend: hayro vs MuPDF (milestone M0)

**Decision: hayro**, with a small patch to its renderer (see `vendor/README.md`).
MuPDF also works, but it is AGPL-licensed and needs a C toolchain for every
target; hayro is pure Rust under Apache-2.0 / MIT and passed every check below.
A MuPDF backend can still be added behind a cargo feature later; the `Doc`
trait in `sshpdf-doc` keeps that possible (see "MuPDF as a fallback").

Spike date: 2026-11-05. Versions: `hayro` 0.9.0 (+ `hayro-interpret` /
`hayro-syntax` 0.9.0, `hayro-font` 0.5.0), `mupdf` crate 0.8.0 (bundles MuPDF
C sources via `mupdf-sys` 0.8.0). Machine: x86_64 Linux, 16 cores.

## Method

Two throwaway programs (one per backend) ran over the corpus (`corpus/`,
pages 1 and 2 of each file). For each page they extracted text with
positions and fonts, walked the vector paths, and rendered the page at
960 px width (about 8 samples per cell at 120 columns). Builds were checked
for the static musl target and for Windows (cross-compiled with mingw, run
under wine).

## Results

| | hayro 0.9 | MuPDF (mupdf 0.8) |
|---|---|---|
| License | Apache-2.0 OR MIT | AGPL-3.0 (or commercial from Artifex) |
| Build | pure Rust; ~30 s release build on 16 cores | C library + bindgen (needs clang); ~32 s wall, 4 min CPU |
| Static musl (`x86_64-unknown-linux-musl`) | builds, static-pie binary runs | **fails** without a musl C cross toolchain (`x86_64-linux-musl-gcc`) |
| Windows (`x86_64-pc-windows-gnu`, mingw, wine) | builds and runs | builds and runs |
| Text: Unicode | every corpus glyph mapped (ToUnicode, glyph names, AGL), incl. Type3 CJK | every glyph mapped |
| Text: positions | per-glyph transform → origin, size, direction; advance from `/Widths` | per-char quad, origin, size |
| Text: grouping | none; sshpdf groups glyphs itself (`sshpdf-doc/src/group.rs`) | blocks / lines / chars (`stext`) |
| Text: font name & flags | name/weight/italic for TrueType & CID fonts; **none for Type1/Type3** (sshpdf matches the glyph widths against the page's font dictionaries) | name + bold/italic for all fonts |
| Invisible text (OCR layers) | reported (`DrawMode::Invisible`) | reported |
| Paths | `Device::draw_path`: kurbo path, transform, paint, fill rule / stroke width & dashes | `NativeDevice::fill_path` / `stroke_path` with path walker, CTM, color |
| Render | 7–55 ms per corpus page at 960 px (text pages 7–11 ms, photo pages 40–55 ms) | 2–61 ms (text pages 2–5 ms) |
| Render **without text and selected lines** | renderer device is crate-private → **patched**: `RenderFilter` hook (150 lines), drops glyph runs and subpaths | possible via a `NativeDevice` that forwards to a draw device (not built in the spike) |
| Text extraction time | 2–19 ms per page | 2–30 ms, but ~200 ms on the CJK (Type3) pages |
| Encryption | RC4, AES-128, AES-256 (`Pdf::new_with_password`) | yes |
| Outline / links | not exposed; read from the raw dictionaries (catalog `/Outlines`, `/Annots`, name trees) — implemented in `sshpdf-doc` | `Document::outlines`, `Page::links` |

Observations from the corpus:

- The two-column arXiv paper uses Type1 fonts (Computer Modern / `SF*`
  EC fonts); MuPDF reports their names, hayro doesn't. Matching each glyph
  run's character codes and widths against the page's `/Widths` arrays
  recovers the font (and with it bold/italic) without patching hayro-font.
- MuPDF's font flags are not always better: it reports
  `NimbusRomNo9L-Medi` (bold Times) as not bold; sshpdf's name rules catch
  it (`fontstyle.rs`).
- The Wikipedia CJK PDF draws CJK glyphs with Type3 fonts. Both backends map
  them to Unicode; hayro is ~15× faster on those pages.
- An owner-password-only file opens without a password in both; hayro
  rejects it if a (non-empty) password is passed, so sshpdf always tries the
  empty password first.

## The hayro patch

hayro's renderer (`hayro::Renderer`) is the only `Device` that rasterizes,
and it is private, so there is no way to drop text or lines from outside.
`vendor/hayro` is hayro 0.9.0 with one addition: a `RenderFilter` trait and
`render_into_filtered`, a copy of `render_into` that wraps the renderer in a
device asking the filter about every glyph run and every path (it may drop
the run, drop the path, or pass a path with some subpaths removed). Content
hayro renders on its own (tiling patterns, soft masks, Type3 glyph
procedures) is not filtered. The patch is self-contained and could be
upstreamed; until then the workspace depends on the vendored copy by path.

## MuPDF as a fallback

Not built. If needed: a `mupdf` cargo feature with a second `Backend`
implementation — `stext` for text (map its chars into `RawGlyph`s and reuse
`group.rs`, or use its blocks directly), a `NativeDevice` for paths, and a
forwarding `NativeDevice` for rendering with exclusions. It would make the
whole binary AGPL when enabled, and the musl release builds would need a musl
C toolchain (e.g. `cross`), so it should stay off by default.
