# sshpdf

A PDF viewer that draws with characters and colors only. It needs no sixel, no
kitty graphics and no setup on either end of an SSH connection. It works in
tmux, Windows Terminal and the Windows console.

sshpdf draws each part of a page in the terminal's own terms:

| Page content | Drawn as |
|---|---|
| Text | real terminal characters at their positions, with bold, italic, colors and super/subscripts |
| Table rules, boxes, underlines | box-drawing characters snapped to the cell grid (`─│┌┼━═`) |
| Photos, plots, curves, fills | block glyphs, from half blocks up to Unicode 16 octants, picked cell by cell |
| Fine detail | a lens that magnifies the area under the cursor 2 to 8 times |

By default the page takes your terminal's colors. White paper becomes the
terminal background, black ink becomes the foreground, on light and dark themes
alike. Photos keep their colors. `--paper` shows real white paper.

## Install

```sh
cargo install --path crates/sshpdf     # from a checkout
```

Release builds (`.github/workflows/release.yml`) are static binaries for Linux
(musl, x86_64 and aarch64), Windows and macOS. The code is pure Rust, so
building needs no C toolchain.

## Use

```sh
sshpdf paper.pdf                       # open, resuming at the last page you read
sshpdf paper.pdf --page 3 --mode reflow
sshpdf slides.pdf --zoom page --tier octant
sshpdf --probe                         # test card for the glyphs and colors your terminal shows
sshpdf --probe --save                  # same, and save your choice
sshpdf --text paper.pdf --page 2       # reflowed plain text
sshpdf --dump paper.pdf --page 1 --size 120x40 --tier octant    # ANSI to stdout
sshpdf --dump-png paper.pdf --page 1 --size 120x40 -o page.png  # simulated terminal render
```

There are three modes:

- `layout` (default) puts text where it sits on the page.
- `reflow` wraps text to the screen in reading order. Figures become
  `[figure p3 #2 — press Enter to view]` lines.
- `raster` draws the whole page as an image, for figure-heavy pages.

Zoom is `text` (default, one terminal row per line of body text), `width`,
`page`, or a percentage where 100 means 12 points per row. At text zoom the
grid is widened a little so proportional text fits its lines. Raster mode and
the lens keep the true aspect.

### Keys

| Key | Action |
|---|---|
| `j` `k` `↓` `↑`, wheel | scroll, continuing onto the next or previous page |
| `h` `l` `←` `→` | scroll sideways |
| `Space` `PgDn` / `b` `PgUp` | screen down / up (`Ctrl-d` `Ctrl-u` for half a screen) |
| `J` / `K` | next / previous page |
| `g` / `G` / `12G` | first / last / page 12 |
| `+` `-` `=` | zoom in / out / text zoom |
| `w` / `p` | fit width / fit page |
| `z` | lens on or off. `hjkl` or arrows move it, `+` `-` change magnification |
| `/` `n` `N` | search, next / previous match |
| `o` | outline (table of contents) |
| `Tab` `Enter` | select a link on screen and follow it. External links are shown and copied |
| `y` | copy the page's text with OSC 52, which works over SSH (tmux needs `set -g set-clipboard on`) |
| `r` / `R` | reflow / raster mode on and off |
| `i` / `?` | info (zoom, tier, colors, bytes per frame) / help |
| `q` | quit |

A number before a key repeats it, so `5j` scrolls five lines. Encrypted files
ask for the password, or take `--password`. The view reloads when the file
changes on disk.

## Terminals

sshpdf never queries the terminal, because inside tmux the answers come from
tmux and not from the terminal. It guesses from environment variables and
starts conservatively. `sshpdf --probe` shows what your terminal can do.

| Terminal | Default tier | Colors |
|---|---|---|
| kitty, foot, Ghostty | octant | truecolor |
| WezTerm, VTE (GNOME Terminal, Tilix, ...) | sextant | truecolor |
| Windows Terminal, iTerm2, Alacritty, Konsole, VS Code | quadrant | truecolor |
| Windows console (conhost) | half | 256 |
| Linux console | half | 16 |
| tmux, unknown | half | 256, or truecolor with `COLORTERM=truecolor` |

In tmux, add `set -as terminal-features ',*:RGB'` to `tmux.conf` for truecolor
and make sure `COLORTERM=truecolor` reaches sshpdf. Under tmux, and with
sextants or octants, sshpdf also repositions the cursor after any character
whose width tmux and the terminal might disagree on. A misjudged glyph then
can't shift a row. `--no-robust` turns this off.

Windows Terminal's default font, Cascadia Code 2404 or newer, has all sextants
and octants. Run `sshpdf --probe --save` and pick `octant`.

## Config

The file is `~/.config/sshpdf/config.toml` (the platform config directory), or
whatever `--config` points at. Command-line flags win over the file, and the
file wins over detection.

```toml
tier = "octant"        # ascii, half, quadrant, sextant, octant, braille
shapes = true          # add Legacy Computing wedges and eighth blocks (sextant/octant)
colors = "true"        # true, 256, 16
theme = "dark"         # dark or light terminal background (when COLORFGBG doesn't say)
paper = false
cell_aspect = 0.5      # cell width / height of your font
robust = true
diffusion = true       # error diffusion between cells with 256/16 colors
mode = "layout"
zoom = "text"

[lens]
cols = 40
rows = 12
magnification = 4

[keys]                 # extra bindings; action names are listed in the help (?)
next_page = ["ctrl-n"]
prev_page = ["ctrl-p"]
```

## Known limitations

- Display math in reflow mode comes out as a jumble of symbols. Layout mode
  shows it where it sits on the page.
- Lines of one table cell set on different baselines can land on two rows.
- Search scans every page synchronously. That is fine up to a few hundred pages.
- Scrolling resends the whole screen, because it doesn't use terminal scroll
  regions. Over a slow link, photo-heavy pages scroll slower than text.
- Colored underlines are not supported, and there is no OCR for scanned pages.
- Windows output is only tested under wine. It has not been checked in a real
  Windows Terminal or conhost.
- Snapshot tests of rendered pages run on x86_64 only, since anti-aliasing can
  differ on aarch64.

## How it works

Five crates:

- `sshpdf-doc` defines the PDF backend trait and implements it on
  [hayro](https://github.com/LaurenzV/hayro), a pure Rust renderer. It groups
  text with positions and fonts into words, lines and blocks in reading order,
  and reads vector paths, the outline and links. Rendering skips what the other
  layers draw, using a small patch to hayro in `vendor/`. `docs/backend.md`
  explains the choice of hayro.
- `sshpdf-layout` maps page points to cells and builds the text layer, the line
  layer (box drawing), reflow and search, then composes all layers.
- `sshpdf-raster` holds the glyph sets (generated from `data/UnicodeData.txt`),
  the per-cell optimizer in OKLab, quantization to 256 or 16 colors with error
  diffusion, and the mapping of page colors to the terminal theme.
- `sshpdf-term` does capability detection, the cell grid, SGR output and the
  ratatui backend with the robust output mode.
- `sshpdf` is the binary: CLI, TUI, config, caching and background rendering.

At text zoom, one terminal row per text line with 1:2 cells would leave
proportional text about a third short of columns, and lines would be dropped. So
the grid is widened by the factor most body lines need (up to 2.2) and shifted
vertically so body lines sit mid-row. All layers share this mapping, which is
why images look wider in layout mode.

## Development

```sh
cargo test --workspace                       # unit, snapshot and tmux tests
INSTA_UPDATE=always cargo test -p sshpdf --test snapshots   # accept new snapshots
SSHPDF_TMUX=/path/to/tmux cargo test -p sshpdf --test tmux  # tmux not on PATH
sshpdf --dump-png FILE --page N --size 120x40 -o out.png    # check visuals by eye
SSHPDF_DEBUG=dropped,links,images,sizes sshpdf --dump ...   # details on stderr
SSHPDF_LOG=/tmp/sshpdf.log sshpdf FILE                      # debug log of the viewer
```

The test corpus in `corpus/` (sources and licenses in `corpus/SOURCES.md`) has
a two-column arXiv paper, a manual with tables, slides, a photo brochure, a
scan, encrypted files, an outline with links, Japanese text and rotated pages.

## License

MIT, see `LICENSE`. The bundled DejaVu Sans Mono font, used only by
`--dump-png`, has its own license in `crates/sshpdf/assets/`. The vendored
hayro keeps its own licenses in `vendor/hayro/`.
