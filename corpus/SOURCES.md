# sshpdf test corpus: sources, licenses, build notes

Built on 2026-11-05. Every file here is either freely licensed (CC BY, CC BY-SA),
public domain, or our own CC0 work. Attribution and changes are listed per file
below; keep this file next to the PDFs when redistributing them.

Rebuild from the original downloads (all steps are scripted, see [Scripts](#scripts)):

```sh
python3 -m venv /tmp/venv && /tmp/venv/bin/pip install reportlab pypdf cryptography pillow
scripts/fetch_sources.sh /tmp/sshpdf-src          # downloads + SHA-256 check
PYTHON=/tmp/venv/bin/python scripts/build_corpus.sh /tmp/sshpdf-src
scripts/check_corpus.sh                            # poppler summary of every PDF
```

Everything rebuilds byte-for-byte except `encrypted.pdf` and
`encrypted-owner-only.pdf` (random AES salts and IVs) and `cjk.pdf` (Wikipedia
renders the PDF on demand from the current revision of the article).

## Summary

| File | Pages | Bytes | License | Main use in tests |
|---|---:|---:|---|---|
| `paper-2col.pdf` | 9 | 2,257,319 | CC BY-SA 4.0 | two-column reading order, math, vector plots, links, outline |
| `manual-tables.pdf` | 10 | 113,563 | CC BY 4.0 | ruled tables, register bit-field diagrams (line layer) |
| `slides.pdf` | 10 | 753,629 | CC BY 4.0 | 16:9 slides, small text, photos, chart, diagram |
| `brochure-photos.pdf` | 3 | 3,826,157 | public domain (NASA) | CMYK JPEG photos, rotated title text |
| `scanned.pdf` | 1 | 573,389 | public domain | image-only page, no text layer |
| `encrypted.pdf` | 12 | 229,606 | CC BY 4.0 | AES-256, user password `sshpdf` |
| `encrypted-owner-only.pdf` | 2 | 19,303 | CC0 1.0 | AES-128, empty user password, restricted permissions |
| `outline-links.pdf` | 12 | 207,406 | CC BY 4.0 | outline, named destinations, URI links, CFF fonts without ToUnicode |
| `cjk.pdf` | 7 | 1,451,507 | CC BY-SA 4.0 (+ image licenses) | Japanese text (wide characters), Type 3 fonts |
| `rotated.pdf` | 4 | 850,069 | CC BY-SA 4.0 | `/Rotate` 0, 90, 270, 180 |
| `synthetic.pdf` | 2 | 17,690 | CC0 1.0 | known positions for every element (ground truth in `synthetic.json`) |

Total: about 9.9 MB.

SHA-256 of the committed files:

```
f6f6e24d76586eaf17b93b04d0c114d73cbc96364ba7709ed7a5b855bf145921  paper-2col.pdf
26f599373254effbc9159550f2279aa53fd5caf876e775334cf863272d88527a  manual-tables.pdf
05c8d8b111fc7c31dc79fa215afe0d7c40d4ba55d1ade3a3b0c1ad7f26e31cb9  slides.pdf
a0d8d0f89d8da824fce2c2d4373b287c9f5cabd1e1a58a92de169fba00f9ad4f  brochure-photos.pdf
8acaa208c99d3de6ffffafe1cc44d2c22bba72c60cac74601b4988790cc3cd01  scanned.pdf
fc173f4f3db3ddcffffea751e1cac2bf7c23c21e40bac5037ec2540d51d19d69  encrypted.pdf
6d05d8efb2c73ecfbc28752493718539ec726cc24c54d781d607d62c46e5cf8e  encrypted-owner-only.pdf
9451a95a336c10753ebed292b0da88580b4517fd87f417ef676424cb0a6f7bd8  outline-links.pdf
57c947a486b641716a561674af805e16725c6f11f8dd270d4a32790e309d9cad  cjk.pdf
98680bcf2173f0f503253ebaa097f645aa3b32d1aa4494783a9a411e3274b092  rotated.pdf
ab46b8b560442718ab378639731e755df042fa25cf923d869a776f79a396ea44  synthetic.pdf
9bdee21a7c0acd5f5d5efbfb7bfa02497fa829d86dbb857f3d0342ad4c7c7cd4  synthetic.json
```

## Files

### paper-2col.pdf

- **Work:** Yujun Choi and Robert Joynt, "Interacting Random-field Dipole Defect
  Model for Heating in Semiconductor-based Qubit Devices", arXiv:2308.00711v1
  [quant-ph] (29 Jul 2023); published as Phys. Rev. Research 6, 013168 (2024).
- **Source:** https://arxiv.org/abs/2308.00711v1 (PDF: https://arxiv.org/pdf/2308.00711v1)
- **License:** CC BY-SA 4.0, https://creativecommons.org/licenses/by-sa/4.0/
  (shown on the arXiv abstract page and in the arXiv OAI-PMH `<license>` field;
  v1 is the only version).
- **Changes:** none. Byte-identical to the arXiv PDF.
- **Notes:** REVTeX two-column layout, US Letter, pdfTeX 1.40.25 + hyperref.
  Embedded Type 1 subset fonts (Computer Modern math, cm-super text) with
  ToUnicode; the vector figures bring in TrueType, CID TrueType and Type 1C
  fonts. Display and inline math. Five figures, all vector plots (pages 4, 5,
  7, 8). Ruled TABLE I on page 4. Outline: 13 entries, 2 levels. 55 internal
  links (`/A << /S /GoTo /D (name) >>`, named destinations) and 8 URI links,
  drawn with colored hyperref borders. Page 1 has arXiv's vertical side stamp
  (rotated text, non-embedded Times-Roman).

### manual-tables.pdf

- **Work:** "The RISC-V Instruction Set Manual, Volume II: Privileged
  Architecture", version 20250508 (Ratified), © RISC-V International and the
  contributors listed in the document (it is a derivative of the privileged spec
  1.9.1, © 2010-2017 Andrew Waterman, Yunsup Lee, Rimas Avižienis, David
  Patterson, Krste Asanović).
- **Source:** release `20250508` of https://github.com/riscv/riscv-isa-manual,
  https://github.com/riscv/riscv-isa-manual/releases/download/20250508/riscv-privileged-20250508.pdf
  (221 pages, 1,372,206 bytes, SHA-256 `d0228bbecc76943aaa5685e381e0263a8c750d18b1b0c452b086b0ddd15a4955`).
- **License:** CC BY 4.0, https://creativecommons.org/licenses/by/4.0/ (stated in
  the document's preface and in the repository LICENSE).
- **Changes:** 10 of 221 pages extracted (source pages 17, 18, 20, 21, 22, 23,
  24, 31, 32, 36) with `scripts/extract_pages.py`. Page content is unchanged;
  links and outline entries that pointed to removed pages were dropped, and the
  Producer string notes the extraction.
- **Pages** (output page, source page, printed page number):
  1. src 17 (p. 10): §1.2, Table 1 (small grid table), two admonition boxes
  2. src 18 (p. 11): Table 2, §1.3
  3. src 20 (p. 13): §2.2, start of Table 3: two header rows with spanning cells, full-width section rows
  4. src 21 (p. 14): Table 3 continued (a full page of grid)
  5. src 22 (p. 15): Table 4 (unprivileged CSR addresses)
  6. src 23 (p. 16): Table 5 (supervisor-level CSR addresses)
  7. src 24 (p. 17): Table 6 (hypervisor and VS CSR addresses)
  8. src 31 (p. 24): Chapter 3 heading, `misa` register diagram (Figure 2), Table 9
  9. src 32 (p. 25): Table 10 (26-row table of `misa` extension bits)
  10. src 36 (p. 29): `mstatus`/`mstatush` register bit-field diagrams (Figures 6 to 9): boxes, tick marks, tiny labels
- **Notes:** Asciidoctor PDF (Prawn), A4. All table rules are stroked
  paths. Fonts: TrueType subsets (Atkinson Hyperlegible, JetBrains Mono, M+ 1p,
  Font Awesome 5 Free Solid) with WinAnsi encoding and ToUnicode, plus one
  non-embedded base-14 font. The admonition "i" icons are Font Awesome glyphs, and
  text extraction yields the private-use character U+F05A for them. Outline: 10
  entries, 4 levels deep. 7 internal links (`/Dest` named destinations).

### slides.pdf

- **Work:** Daniel S. Katz, Ben Clifford, Kevin Hunter Kesling, Kyle Chard,
  Yadu Babuji, "Research Software Phases, Sustainability, and RSEs", talk at
  deRSE25 (5th conference for Research Software Engineering in Germany),
  Karlsruhe, 25 February 2025.
- **Source:** Zenodo record 14922563, https://doi.org/10.5281/zenodo.14922563,
  file `Parsl-deRSE25.pdf`
  (https://zenodo.org/records/14922563/files/Parsl-deRSE25.pdf?download=1; 17
  pages, 895,175 bytes, MD5 `beb6f9d8c87980b675e211d79553d7f4` as listed by
  Zenodo, SHA-256 `a52ffaec6f59058ed9436424c7d00aee5ba90ad4e5f3c6327829d166afcce152`).
- **License:** CC BY 4.0, https://creativecommons.org/licenses/by/4.0/ (Zenodo
  record metadata).
- **Changes:** 10 of 17 pages extracted (source pages 1, 3, 4, 5, 7, 8, 9, 11,
  12, 16) with `scripts/extract_pages.py`. Source page 2 was left out on purpose
  because it shows third-party logos (Python, Bash). Content is unchanged.
- **Pages:** 1 title slide (dark photo background, small text); 2 nested
  bullets; 3 bullets + raster bar chart (JPEG); 4 two-column bullets; 5
  "Project stages" chevron diagram (indexed-color images with soft masks); 6
  bullets + an underlined URL; 7 two-column bullets + group photo; 8 and 9 very
  small text (the slides to test the lens on); 10 bullets.
- **Notes:** Keynote export (macOS 15.3.1 Quartz PDFContext), 960 x 540 pt.
  Every slide has a full-size 1920 x 1080 JPEG background (ICC-based color).
  Fonts are TrueType subsets: Calibri and Arial (WinAnsi, ToUnicode), Georgia
  and Georgia-Bold (MacRoman, **no ToUnicode**). 12 URI links (a DOI link in
  each footer, among others). pypdf left an empty `/Outlines` dictionary in the
  catalog (no entries). Third-party marks stay as in the original deck: the
  University of Illinois and NCSA logos in the footer, and the deRSE25
  conference logo and CC BY badge on the title slide. These are trademarks of
  their owners. The group photo on page 7 is part of the CC BY deck (no
  separate photo credit is given).

### brochure-photos.pdf

- **Work:** NASA Facts "Hangar AE", Launch Services Program, NASA Kennedy Space
  Center, 2023 (NASA ID FS-2023-03-273-KSC).
- **Source:** https://www.nasa.gov/wp-content/uploads/2023/10/hangar-ae-fact-sheet.pdf,
  linked from https://www.nasa.gov/reference/lsp-media-resources/. The same file
  (same SHA-1) is on Wikimedia Commons:
  https://commons.wikimedia.org/wiki/File:Hangar_AE_Fact_Sheet.pdf (tagged PD-USGov-NASA).
- **License:** public domain. It is a work of the U.S. federal government (17
  U.S.C. § 105); see NASA's media guidelines,
  https://www.nasa.gov/nasa-brand-center/images-and-media/. Every photo credit
  printed in the document is "NASA", "NASA/Ben Smegelsky" or "U.S. Air Force",
  all U.S. government works. The NASA insignia on page 1 is not copyrighted,
  but its use is restricted (14 CFR Part 1221): don't use it to suggest NASA
  endorses anything.
- **Changes:** none. Byte-identical to the nasa.gov file.
- **Notes:** Adobe InDesign 18.2 / Adobe PDF Library 17, US Letter. Nine
  photos are **DeviceCMYK JPEGs with an Adobe APP14 marker** (the usual
  inverted-CMYK pitfall in JPEG decoders). There is one indexed-color image.
  Fonts: Helvetica Neue LT Std as Type 1C (CFF) subsets. The large "NASAfacts"
  title runs vertically (rotated text). Also a photo timeline grid with
  captions and a "Quick Facts" box.

### scanned.pdf

- **Work:** Oliver Byrne, "The First Six Books of the Elements of Euclid, in
  which Coloured Diagrams and Symbols are Used Instead of Letters for the
  Greater Ease of Learners", London: William Pickering, 1847. The page is Book
  I, Proposition I.
- **Source:** scan by Smithsonian Libraries, Internet Archive item
  `firstsixbooksel00byrn`, https://archive.org/details/firstsixbooksel00byrn.
  Page image: https://archive.org/download/firstsixbooksel00byrn/page/n38.jpg
  (leaf n38; 2828 x 3777 px JPEG, 572,181 bytes, SHA-256
  `d2515af833dd568a7ed60c74b6613c66d016213fb924dab9477362d00c67c251`).
- **License:** public domain (https://creativecommons.org/publicdomain/mark/1.0/).
  The 1847 work is long out of copyright, and the item's rights statement reads:
  "Public domain. The Library considers that this work is no longer under
  copyright protection". A faithful photograph of a 2-D public-domain page adds
  no new copyright in the U.S.
- **Changes:** the JPEG was wrapped, byte-for-byte unchanged, into a one-page PDF
  by `scripts/make_scanned.py`. The page is 678.72 x 906.48 pt (the image at
  300 dpi), with one `/DCTDecode` DeviceRGB image XObject. There are **no fonts
  and no text operators**; `pdftotext` returns nothing.
- **Notes:** color scan with toned paper and show-through, a decorative header
  band, a drop cap, red/yellow/blue geometric diagrams and body text (useful
  later for OCR).

### encrypted.pdf

- **Work / license:** a copy of `outline-links.pdf` (Yingru Li, CC BY 4.0,
  https://creativecommons.org/licenses/by/4.0/, see below).
- **Changes:** encrypted with `scripts/make_encrypted.py`.
- **Encryption:** Standard security handler, `/V 5 /R 6`, AES-256 (`/CFM /AESV3`).
  **User password `sshpdf`**, owner password `sshpdf-owner`. All permissions
  granted (`/P -4`). Opening without a password fails
  (`pdfinfo encrypted.pdf` → "Incorrect password"); `pdfinfo -upw sshpdf` works.
- **Notes:** strings are encrypted as well as streams: outline titles, URIs and
  the named-destination keys in the `/Names /Dests` tree must all be decrypted
  before link targets resolve. It has the same 12 pages, outline and links as
  `outline-links.pdf`.

### encrypted-owner-only.pdf

- **Work / license:** a copy of `synthetic.pdf` (CC0 1.0).
- **Changes:** encrypted with `scripts/make_encrypted.py`.
- **Encryption:** `/V 4 /R 4`, AES-128 (`/CFM /AESV2`). **Empty user
  password**, so it opens without a prompt. Owner password `sshpdf-owner`.
  Permissions: printing allowed; modifying, copying text and adding annotations
  are **denied** (`/P -60`). Poppler reports `copy:no` but `pdftotext` still
  extracts the text.

### outline-links.pdf

- **Work:** Yingru Li, "Probability Tools for Sequential Random Projection",
  arXiv:2402.14026v3 [math.ST] (12 May 2024).
- **Source:** https://arxiv.org/abs/2402.14026v3 (PDF: https://arxiv.org/pdf/2402.14026v3)
- **License:** CC BY 4.0, https://creativecommons.org/licenses/by/4.0/ (arXiv
  abstract page).
- **Changes:** none. Byte-identical to the arXiv PDF.
- **Notes:** made by GPL Ghostscript 10.01.2 (arXiv's dvips route), so it is a
  different producer from `paper-2col.pdf`. US Letter, single column. Fonts are
  embedded **Type 1C (CFF) subsets, many without ToUnicode**: text extraction
  has to go through the font's encoding and glyph names. Outline: 11 entries, 2
  levels (some titles contain raw LaTeX labels, e.g. "Proof of thm:seq-jl").
  99 internal links written as `/Dest (name)`, resolved through the
  `/Names /Dests` name tree (101 named destinations). 5 URI links, including
  the arXiv stamp link. Page 2 has a vector diagram.

### cjk.pdf

- **Work:** the Japanese Wikipedia article 回文 ("palindrome"), revision
  111190598 (2026-09-28T03:21:41Z), by Wikipedia contributors:
  https://ja.wikipedia.org/w/index.php?title=%E5%9B%9E%E6%96%87&oldid=111190598
  (authors: https://ja.wikipedia.org/w/index.php?title=%E5%9B%9E%E6%96%87&action=history).
- **Source:** Wikimedia REST API PDF render,
  https://ja.wikipedia.org/api/rest_v1/page/pdf/%E5%9B%9E%E6%96%87, fetched
  2026-11-05. The PDF's footer names the revision it was rendered from
  (`oldid=111190598`).
- **License:** text CC BY-SA 4.0, https://creativecommons.org/licenses/by-sa/4.0/.
  The images in the PDF are all freely licensed Wikimedia Commons files:
  - "Sator Square at Oppède.jpg" by M Disdero, CC BY-SA 3.0
    (https://creativecommons.org/licenses/by-sa/3.0/),
    https://commons.wikimedia.org/wiki/File:Sator_Square_at_Opp%C3%A8de.jpg
  - "Ambigram palindrome ΝΙΨΟΝΑΝΟΜΗΜΑΤΑΜΗΜΟΝΑΝΟΨΙΝ (Wash your sins, not only
    your face, in Greek).jpg" by Christina Kekka from Athens, Greece (light
    correction by Basile Morin), CC BY 2.0 (https://creativecommons.org/licenses/by/2.0/),
    https://commons.wikimedia.org/wiki/File:Ambigram_palindrome_%CE%9D%CE%99%CE%A8%CE%9F%CE%9D%CE%91%CE%9D%CE%9F%CE%9C%CE%97%CE%9C%CE%91%CE%A4%CE%91%CE%9C%CE%97%CE%9C%CE%9F%CE%9D%CE%91%CE%9D%CE%9F%CE%A8%CE%99%CE%9D_(Wash_your_sins,_not_only_your_face,_in_Greek).jpg
  - "Al roverso symfonie 47 Haydn.png" by Quatrostein, CC BY-SA 3.0,
    https://commons.wikimedia.org/wiki/File:Al_roverso_symfonie_47_Haydn.png
  - the Wikipedia logo (Wikipedia-logo-v2.svg: version 1 by Nohat, concept by
    Paullusmagnus; Wikimedia; CC BY-SA 3.0) and the Japanese wordmark/tagline
    (public domain). "Wikipedia" and its logos are trademarks of the Wikimedia
    Foundation.
  - a 1 x 1 px image on page 7.
- **Changes:** none. This is the PDF as served.
- **Notes:** HeadlessChrome 154 / Skia/PDF m154, A4, tagged PDF. **The CJK glyphs
  are in Type 3 fonts:** Skia turns the CFF-based Noto Sans CJK JP and Noto
  Serif CJK JP (Regular and Bold) into 263 Type 3 font chunks with ToUnicode.
  Their FontMatrix is y-flipped (`[0.001 0 0 -0.001 0 0]`). Latin text uses
  Type0 / CIDFontType2 fonts with `Identity-H` and ToUnicode (Liberation Serif,
  Liberation Serif Bold, DejaVu Sans). The text mixes kanji, hiragana, katakana,
  full-width punctuation, Latin, Greek and IPA, and wraps around floated images.
  222 URI links and 22 internal links (`/Dest (name)` resolved through the
  catalog's `/Dests` dictionary). `pdftotext` extracts the Japanese correctly,
  with no private-use or U+FFFD characters.

### rotated.pdf

- **Work / license:** pages from `paper-2col.pdf` (Yujun Choi and Robert Joynt,
  CC BY-SA 4.0, https://creativecommons.org/licenses/by-sa/4.0/). Following the
  ShareAlike term, this derivative is also CC BY-SA 4.0.
- **Changes:** `scripts/make_rotated.py` copied 4 pages and set `/Rotate` on 3 of them:

  | Output page | Source page of paper-2col.pdf | `/Rotate` |
  |---:|---:|---:|
  | 1 | 1 | none (0) |
  | 2 | 2 | 90 |
  | 3 | 4 (figure + TABLE I) | 270 |
  | 4 | 5 (figure) | 180 |

- **Notes:** only `/Rotate` changed. Content streams, link rectangles and
  paths stay in unrotated page space. Kept: 7 outline entries, 5 internal GoTo
  links and 1 URI link.

### synthetic.pdf (and synthetic.json)

- **Work:** our own, generated by `gen_synthetic.py`. **License:** CC0 1.0,
  https://creativecommons.org/publicdomain/zero/1.0/.
- **Notes:** deterministic (reportlab invariant mode). Two US Letter pages. All
  fonts are non-embedded base-14 Type 1 fonts (Times-Roman/Italic/Bold,
  Helvetica/Bold, Courier). Content streams use `/ASCII85Decode /FlateDecode`
  (reportlab's default). Outline: 7 entries, 2 levels. One internal link with
  an explicit destination (`/Dest [page2 /Fit]`) and one URI link.
  `synthetic.json` lists every element (103 items: text runs with font, size,
  baseline origin, advance width, color and Tc/Tw/Tz/rise; lines; rectangles;
  the circle; the image; the links).

  Coordinates are PDF points, origin bottom-left, y up; text positions are the
  baseline start.

  Page 1:

  | Element | Position |
  |---|---|
  | Title, Helvetica-Bold 20 | baseline (72, 720) |
  | Subtitle, Helvetica 9, gray 0.35 | (72, 704) |
  | 5 body lines, Times-Roman 10 pt on 12 pt leading | x 72, y 680, 668, 656, 644, 632 (punctuation and accented Latin on 644 and 632) |
  | Times-Italic line | (72, 620) |
  | Times-Bold line | (72, 608) |
  | Red line, DeviceRGB (1, 0, 0) | (72, 596) |
  | Superscripts and subscripts | (72, 584). Script glyphs are Times-Roman 7 pt; superscripts raised 3.5 pt with `Ts`, subscripts lowered 2 pt with `Td` |
  | Ligature line | (72, 572). /Times-Roman with `/Differences [129 /fi 141 /fl]`, no ToUnicode |
  | Letter spacing, Tc 1.5 | (72, 560) |
  | Horizontal scaling, Tz 80 | (72, 548) |
  | Table, 4 rows x 3 columns | outer box x 72..382, y 458..530, 1 pt, drawn with `re`; inner rules 0.5 pt, drawn with `m`/`l`: horizontal at y 512, 494, 476, vertical at x 222, 302. Cell text Helvetica 9 (header row bold); numeric columns right-aligned |
  | Navy filled box with white Helvetica-Bold 10 text | box x 400..540, y 476..512; text at (410, 490) |
  | 3 pt horizontal rule | y 440, x 72..440 |
  | Double rule: two 0.5 pt lines with centers 2 pt apart | y 424 and 422, x 72..440 |
  | Diagonal line, 1 pt | (72, 400) to (172, 320) |
  | Circle, 1 pt stroke | center (240, 360), r 36 (4 Bézier curves) |
  | Light-gray fill, gray 0.85, no stroke | x 300..390, y 324..396 |
  | RGB gradient image, 64 x 64 px | x 420..484, y 330..394; pixel (r, g, b) = (4·col, 4·row, 128), row 0 at the top |
  | Size ladder, Times-Roman 6/8/12/16 pt | y 282, 270, 254, 234 |
  | Courier 10 line | (72, 214) |
  | Internal link to page 2 / URI link | text at y 196 / 184; link rectangles in `synthetic.json` |
  | Text rotated 90° counter-clockwise (via `cm`, not `/Rotate`) | baseline starts at (575, 330) and runs up |
  | Footer, Helvetica 8 | (72, 40) |

  Page 2: heading Helvetica-Bold 14 at (72, 720); italic note at (72, 704).
  Two columns of Times-Roman 10 pt on 12 pt leading, first baselines at y 680:
  the left column (x 72..287) is justified with word spacing `Tw`; the right
  column (x 325..540) is ragged right. Paragraphs are separated by one empty
  line. The word "information" is split across a line break in the left column
  ("infor-" / "mation"). The word "correct" is split across the column break:
  "cor-" ends the left column and "rect" starts the right column. A real hyphen
  ("two-column") also ends up mid-line. A 0.5 pt footnote rule runs at y 100,
  x 72..200, with a Times-Roman 8 footnote at (72, 88).

## Scripts

All scripts are CC0 1.0. Python scripts take local paths only and never touch
the network.

| Script | What it does |
|---|---|
| `gen_synthetic.py` | writes `synthetic.pdf` and `synthetic.json` (reportlab) |
| `scripts/fetch_sources.sh DIR` | downloads all original sources and checks their SHA-256 |
| `scripts/build_corpus.sh SRC [OUT]` | builds every corpus file from the downloads (page lists, passwords and rotations live here) |
| `scripts/extract_pages.py SRC OUT PAGES` | page extraction with pypdf `append()`; keeps links, named destinations and outline entries that still resolve |
| `scripts/make_scanned.py SRC.jpg OUT.pdf` | hand-written one-page PDF around an unchanged JPEG (no fonts, no text) |
| `scripts/make_encrypted.py SRC OUT ...` | Standard-handler encryption (AES-256 / AES-128 / RC4) with chosen passwords and permissions |
| `scripts/make_rotated.py SRC OUT PAGE:ANGLE...` | copies pages and sets `/Rotate` |
| `scripts/check_corpus.sh [DIR]` | poppler summary of every PDF; fails if `scanned.pdf` has text or another file has none |

Tool versions used: Python 3.14.9, reportlab 5.1.0, pypdf 6.21.4,
cryptography 50.0.2, Pillow 13.0.0; checked with poppler 26.10.0 (`pdfinfo`,
`pdftotext`, `pdffonts`, `pdfimages`).

Two pypdf details are handled in the scripts. pypdf writes `/P` as an unsigned
number (e.g. `4294967292`), so `make_encrypted.py` stores the equivalent signed
32-bit value (`-4`) that other writers use. Page extraction leaves an empty
`/Outlines` dictionary when the source had none (see `slides.pdf`).

## Coverage notes and gaps

- **Font types:** Type 1 embedded (paper), Type 1C/CFF (outline-links,
  brochure), simple TrueType (manual, slides, paper figures), Type0/CIDFontType2
  `Identity-H` (cjk Latin text, paper figures), Type 3 (cjk), non-embedded
  base-14 (synthetic, arXiv stamps, manual). Missing ToUnicode: outline-links
  (CFF), slides (Georgia, MacRoman), synthetic (base-14, incl. the `/Differences`
  ligature font).
- **Destinations:** `/A /GoTo` with a named destination (paper-2col, rotated);
  `/Dest (name)` through the `/Names /Dests` tree (outline-links, encrypted,
  manual-tables); `/Dest (name)` through the catalog `/Dests` dictionary (cjk);
  explicit `/Dest [page /Fit]` (synthetic).
- **Images:** CMYK JPEG with Adobe APP14 (brochure), RGB JPEG with ICC (slides,
  cjk), soft masks and indexed images (slides), Flate RGB (synthetic),
  full-page scan (scanned).
- **Not covered:** CJK text in Type0/CID fonts (in `cjk.pdf` the CJK glyphs are
  Type 3); vertical (tategaki) writing; bilevel CCITT/JBIG2 scans; RC4
  encryption; form fields.
