#!/usr/bin/env python3
"""Generate corpus/synthetic.pdf and corpus/synthetic.json for sshpdf tests.

License: CC0 1.0 Universal. This script and everything it generates are
dedicated to the public domain (https://creativecommons.org/publicdomain/zero/1.0/).

Requires reportlab (tested with reportlab 5.1.0, Python 3.14). reportlab runs in
"invariant" mode, so the output is byte-for-byte identical on every run with the
same reportlab version.

Usage:
    python3 gen_synthetic.py [--out synthetic.pdf] [--json synthetic.json]

Coordinates are PDF user-space points: origin at the BOTTOM-LEFT corner of the
page, x to the right, y up. Both pages are US Letter, 612 x 792 pt. For text,
(x, y) is the start of the baseline. Every element drawn is also written to
synthetic.json with its exact geometry (that file is the full ground truth; the
constants below are the source of both).

Page 1: text samples, a ruled table, rules and shapes
  y=720  title, Helvetica-Bold 20, x=72
  y=704  subtitle, Helvetica 9, gray
  y=680..548  body lines, x=72, one every 12 pt (Times-Roman 10 pt, 12 pt
         leading), then special lines at the same pitch:
           620 Times-Italic, 608 Times-Bold, 596 red (DeviceRGB 1 0 0),
           584 superscripts (7 pt, raised 3.5 pt with Ts) and subscripts
               (7 pt, baseline lowered 2 pt with Td),
           572 "fi"/"fl" ligatures (Times-Roman with an /Encoding /Differences
               array: code 129 -> /fi, code 141 -> /fl; no ToUnicode),
           560 letter spacing (Tc 1.5), 548 horizontal scaling (Tz 80)
  table  4 rows x 3 columns, outer box x=72..382, y=458..530 (1 pt, drawn
         with "re"), inner rules 0.5 pt (drawn with "m/l"): horizontal at
         y=512, 494, 476; vertical at x=222, 302. Cell text Helvetica 9
         (header row Helvetica-Bold 9).
  navy box x=400..540, y=476..512, white Helvetica-Bold 10 text on it
  y=440  3 pt horizontal rule, x=72..440
  y=424 and y=422  double rule: two 0.5 pt lines, centers 2 pt apart, x=72..440
  diagonal 1 pt line (72,400)-(172,320); circle center (240,360) r=36, 1 pt;
  light-gray filled rectangle x=300..390, y=324..396 (gray 0.85, no stroke);
  64x64 px RGB gradient image placed at x=420..484, y=330..394
  size ladder: 6, 8, 12, 16 pt Times-Roman lines at y=282, 270, 254, 234
  y=214  Courier 10; y=196 internal link to page 2 (explicit /Dest array);
         y=184 URI link
  text rotated 90 degrees counter-clockwise starting at (575, 330)
Page 2: two-column text (Times-Roman 10 pt, 12 pt leading)
  left column x=72..287, justified with word spacing (Tw); right column
  x=325..540, ragged right; first baselines at y=680 in both columns.
  "infor-"/"mation" is split across a line break inside the left column, and
  "cor-"/"rect" is split across the column break (end of the left column ->
  top of the right column). A footnote (Times-Roman 8) sits under a short
  0.5 pt rule at the bottom.
Both pages: footer in Helvetica 8 at y=40. An outline (bookmarks, 2 levels)
points at the sections.
"""

import argparse
import codecs
import encodings.cp1252
import json
import os

from reportlab.lib.pagesizes import letter
from reportlab.lib.utils import ImageReader
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.pdfmetrics import stringWidth
from reportlab.pdfgen import canvas

PAGE_W, PAGE_H = letter  # 612 x 792 pt
LEFT = 72.0

# Times-Roman with WinAnsiEncoding plus /Differences [129 /fi 141 /fl].
# reportlab encodes Type 1 text with a Python codec of the same name as the
# encoding, so a matching codec is registered (cp1252 + the two ligatures).
LIG_ENCODING = "sshpdf_winansi_lig"
LIG_FONT = "Times-Roman-Lig"  # reportlab-internal name; /BaseFont stays /Times-Roman
LIG_CODES = {"ﬁ": (0x81, "fi"), "ﬂ": (0x8D, "fl")}

SUP_RISE = 3.5  # superscript baseline offset (Ts)
SUB_SHIFT = -2.0  # subscript baseline offset (Td)
SCRIPT_SIZE = 7.0

COL_W = 215.0
COL_X = (72.0, 325.0)


def register_ligature_font():
    table = list(encodings.cp1252.decoding_table)
    for ch, (code, _glyph) in LIG_CODES.items():
        table[code] = ch
    decoding = "".join(table)
    encoding = codecs.charmap_build(decoding)

    def search(name):
        if name.replace("-", "_") != LIG_ENCODING:
            return None
        return codecs.CodecInfo(
            name=LIG_ENCODING,
            encode=lambda s, errors="strict": codecs.charmap_encode(s, errors, encoding),
            decode=lambda b, errors="strict": codecs.charmap_decode(b, errors, decoding),
        )

    codecs.register(search)
    enc = pdfmetrics.Encoding(LIG_ENCODING, "WinAnsiEncoding")
    for _ch, (code, glyph) in LIG_CODES.items():
        enc[code] = glyph
    pdfmetrics.registerEncoding(enc)
    pdfmetrics.registerFont(pdfmetrics.Font(LIG_FONT, "Times-Roman", LIG_ENCODING))


def r3(v):
    return round(float(v), 3)


class Recorder:
    """Collects the ground truth written to synthetic.json."""

    def __init__(self):
        self.items = []

    def add(self, **kw):
        for k, v in list(kw.items()):
            if isinstance(v, float):
                kw[k] = r3(v)
            elif isinstance(v, tuple):
                kw[k] = [r3(x) if isinstance(x, (int, float)) else x for x in v]
        self.items.append(kw)


def base_font_name(font):
    return "Times-Roman" if font == LIG_FONT else font


def text_line(c, rec, page, x, y, s, font="Times-Roman", size=10.0, color=(0, 0, 0),
              tc=0.0, tz=100.0, tw=0.0, role="body"):
    """One line in its own BT/ET, wrapped in q/Q so Tc/Tw/Tz don't leak."""
    c.saveState()
    t = c.beginText(x, y)
    t.setFont(font, size)
    t.setFillColorRGB(*color)
    if tc:
        t.setCharSpace(tc)
    if tz != 100.0:
        t.setHorizScale(tz)
    if tw:
        t.setWordSpace(tw)
    t.textOut(s)
    c.drawText(t)
    c.restoreState()
    # Advance width: sum of (glyph width + Tc [+ Tw for spaces]) scaled by Tz.
    adv = (stringWidth(s, font, size) + tc * len(s) + tw * s.count(" ")) * tz / 100.0
    item = dict(kind="text", page=page, role=role, text=s, font=base_font_name(font),
                size=float(size), x=float(x), y=float(y), advance=adv, color=tuple(color))
    if tc:
        item["char_spacing_Tc"] = float(tc)
    if tz != 100.0:
        item["horiz_scale_Tz"] = float(tz)
    if tw:
        item["word_spacing_Tw"] = float(tw)
    if font == LIG_FONT:
        item["encoding"] = "WinAnsiEncoding + Differences [129 /fi 141 /fl]"
        item["text_expanded"] = s.replace("ﬁ", "fi").replace("ﬂ", "fl")
    rec.add(**item)
    return x + adv


def scripts_line(c, rec, page, x, y, runs):
    """A single text object: superscripts via Ts (rise), subscripts via Td."""
    t = c.beginText(x, y)
    cx = x  # current point
    line_x = x  # start of the current text line; Td is relative to it
    for s, kind in runs:
        size = 10.0 if kind == "normal" else SCRIPT_SIZE
        t.setFont("Times-Roman", size)
        w = stringWidth(s, "Times-Roman", size)
        if kind == "sup":
            t.setRise(SUP_RISE)
            t.textOut(s)
            t.setRise(0)
            base = y + SUP_RISE
        elif kind == "sub":
            # reportlab's moveCursor(dx, dy) emits "dx -dy Td".
            t.moveCursor(cx - line_x, -SUB_SHIFT)  # down to the subscript baseline
            t.textOut(s)
            t.moveCursor(w, SUB_SHIFT)  # back up, just after the subscript
            line_x = cx + w
            base = y + SUB_SHIFT
        else:
            t.textOut(s)
            base = y
        mech = {"sup": "Ts (text rise) +3.5", "sub": "Td baseline shift -2", "normal": None}[kind]
        item = dict(kind="text", page=page, role="script-" + kind, text=s, font="Times-Roman",
                    size=size, x=cx, y=base, advance=w, color=(0, 0, 0))
        if mech:
            item["mechanism"] = mech
        rec.add(**item)
        cx += w
    c.drawText(t)
    return cx


def stroke_line(c, rec, page, x1, y1, x2, y2, width, role):
    c.setLineWidth(width)
    c.line(x1, y1, x2, y2)
    rec.add(kind="line", page=page, role=role, x1=float(x1), y1=float(y1), x2=float(x2),
            y2=float(y2), stroke_width=float(width), color=(0, 0, 0), op="m l S")


def gradient_image(n=64):
    from PIL import Image

    img = Image.new("RGB", (n, n))
    px = img.load()
    for yy in range(n):
        for xx in range(n):
            # red grows left->right, green grows top->bottom, blue constant
            px[xx, yy] = (xx * 4, yy * 4, 128)
    return img


def footer(c, rec, page, total):
    s = f"sshpdf corpus: synthetic.pdf, page {page} of {total}. Generated by gen_synthetic.py, CC0 1.0."
    text_line(c, rec, page, LEFT, 40, s, font="Helvetica", size=8, color=(0.3, 0.3, 0.3), role="footer")


def page1(c, rec):
    p = 1
    c.bookmarkPage("p1")
    c.addOutlineEntry("Page 1: text, table and shapes", "p1", level=0)
    text_line(c, rec, p, LEFT, 720, "sshpdf synthetic test page", font="Helvetica-Bold", size=20, role="title")
    text_line(c, rec, p, LEFT, 704,
              "Generated by corpus/gen_synthetic.py (CC0). Coordinates: PDF points, origin bottom-left.",
              font="Helvetica", size=9, color=(0.35, 0.35, 0.35), role="subtitle")

    c.bookmarkHorizontalAbsolute("p1-text", 692)
    c.addOutlineEntry("Text samples", "p1-text", level=1)
    body = [
        "The quick brown fox jumps over the lazy dog. 0123456789",
        "Body text: Times-Roman 10 pt on 12 pt leading, left edge at x = 72 pt.",
        "Each baseline is 12 pt below the one above it; this is the third line.",
        "Punctuation: (round) [square] {curly} “double” ‘single’ – en — em … ellipsis",
        "Accented Latin: café, naïve, Zürich, São Paulo, Ångström, œuvre, Æsop, façade.",
    ]
    y = 680.0
    for s in body:
        text_line(c, rec, p, LEFT, y, s)
        y -= 12
    text_line(c, rec, p, LEFT, 620, "This line is set in Times-Italic 10 pt.", font="Times-Italic", role="italic")
    text_line(c, rec, p, LEFT, 608, "This line is set in Times-Bold 10 pt.", font="Times-Bold", role="bold")
    text_line(c, rec, p, LEFT, 596, "This line is red: DeviceRGB (1, 0, 0).", color=(1, 0, 0), role="red")
    scripts_line(c, rec, p, LEFT, 584, [
        ("Scripts: E = mc", "normal"), ("2", "sup"), (" and x", "normal"), ("2", "sup"),
        (" + y", "normal"), ("2", "sup"), (" = z", "normal"), ("2", "sup"),
        (" (superscripts); H", "normal"), ("2", "sub"), ("O and CO", "normal"), ("2", "sub"),
        (" (subscripts).", "normal"),
    ])
    text_line(c, rec, p, LEFT, 572,
              "Ligatures: ﬁnd the ﬂow of ofﬁce ﬁles (ﬁ is glyph /fi, ﬂ is glyph /fl).",
              font=LIG_FONT, role="ligatures")
    text_line(c, rec, p, LEFT, 560, "Letter-spaced with Tc = 1.5 pt.", tc=1.5, role="char-spacing")
    text_line(c, rec, p, LEFT, 548, "Horizontally scaled to 80 percent with Tz = 80.", tz=80.0, role="horiz-scale")

    # ---- table: 4 rows x 3 columns
    c.bookmarkHorizontalAbsolute("p1-table", 540)
    c.addOutlineEntry("Table (4 rows x 3 columns)", "p1-table", level=1)
    tx0, ty_top, col_w, row_h = 72.0, 530.0, (150.0, 80.0, 80.0), 18.0
    tx1 = tx0 + sum(col_w)
    ty_bot = ty_top - 4 * row_h
    c.setLineWidth(1.0)
    c.rect(tx0, ty_bot, tx1 - tx0, ty_top - ty_bot, stroke=1, fill=0)
    rec.add(kind="rect", page=p, role="table-outer-box", x=tx0, y=ty_bot, w=tx1 - tx0,
            h=ty_top - ty_bot, stroke_width=1.0, fill=None, color=(0, 0, 0), op="re S")
    for i in range(1, 4):
        yy = ty_top - i * row_h
        stroke_line(c, rec, p, tx0, yy, tx1, yy, 0.5, "table-inner-horizontal")
    xx = tx0
    for wcol in col_w[:-1]:
        xx += wcol
        stroke_line(c, rec, p, xx, ty_bot, xx, ty_top, 0.5, "table-inner-vertical")
    rows = [("Item", "Qty", "Price"), ("Widget", "4", "3.50"), ("Gadget", "12", "17.25"),
            ("Gizmo, large", "1", "120.00")]
    for ri, row in enumerate(rows):
        base = ty_top - ri * row_h - 12.5
        font = "Helvetica-Bold" if ri == 0 else "Helvetica"
        cx = tx0
        for ci, cell in enumerate(row):
            if ci == 0:
                x = cx + 4
            else:  # numeric columns are right-aligned, 4 pt inset
                x = cx + col_w[ci] - 4 - stringWidth(cell, font, 9)
            text_line(c, rec, p, x, base, cell, font=font, size=9,
                      role="table-header" if ri == 0 else "table-cell")
            cx += col_w[ci]

    c.setFillColorRGB(0.1, 0.15, 0.4)
    c.rect(400, 476, 140, 36, stroke=0, fill=1)
    rec.add(kind="rect", page=p, role="navy-fill", x=400.0, y=476.0, w=140.0, h=36.0,
            stroke_width=None, fill=(0.1, 0.15, 0.4), op="re f")
    text_line(c, rec, p, 410, 490, "White text on navy", font="Helvetica-Bold", size=10,
              color=(1, 1, 1), role="text-on-fill")
    c.setFillColorRGB(0, 0, 0)

    # ---- rules
    c.bookmarkHorizontalAbsolute("p1-shapes", 450)
    c.addOutlineEntry("Rules and shapes", "p1-shapes", level=1)
    stroke_line(c, rec, p, 72, 440, 440, 440, 3.0, "thick-rule-3pt")
    text_line(c, rec, p, 450, 437, "3 pt rule", font="Helvetica", size=8, role="label")
    stroke_line(c, rec, p, 72, 424, 440, 424, 0.5, "double-rule-upper")
    stroke_line(c, rec, p, 72, 422, 440, 422, 0.5, "double-rule-lower")
    text_line(c, rec, p, 450, 420, "double rule, 2 x 0.5 pt", font="Helvetica", size=8, role="label")

    # ---- shapes
    stroke_line(c, rec, p, 72, 400, 172, 320, 1.0, "diagonal")
    text_line(c, rec, p, 72, 306, "diagonal line", font="Helvetica", size=8, role="label")
    c.setLineWidth(1.0)
    c.circle(240, 360, 36, stroke=1, fill=0)
    rec.add(kind="circle", page=p, role="circle", cx=240.0, cy=360.0, r=36.0, stroke_width=1.0,
            color=(0, 0, 0), op="4 Bezier curves (c), S")
    text_line(c, rec, p, 204, 306, "circle, r = 36 pt", font="Helvetica", size=8, role="label")
    c.setFillGray(0.85)
    c.rect(300, 324, 90, 72, stroke=0, fill=1)
    rec.add(kind="rect", page=p, role="light-gray-fill", x=300.0, y=324.0, w=90.0, h=72.0,
            stroke_width=None, fill=(0.85, 0.85, 0.85), colorspace="DeviceGray 0.85", op="re f")
    c.setFillGray(0)
    text_line(c, rec, p, 300, 306, "gray 0.85 fill", font="Helvetica", size=8, role="label")
    c.drawImage(ImageReader(gradient_image()), 420, 330, width=64, height=64)
    rec.add(kind="image", page=p, role="rgb-gradient", x=420.0, y=330.0, w=64.0, h=64.0,
            px_w=64, px_h=64, colorspace="DeviceRGB", bits=8,
            pixel="(r, g, b) = (4*col, 4*row, 128), row 0 = top")
    text_line(c, rec, p, 420, 306, "64 x 64 px RGB image", font="Helvetica", size=8, role="label")

    # ---- size ladder, monospace, links
    for size, yy in ((6, 282), (8, 270), (12, 254), (16, 234)):
        text_line(c, rec, p, LEFT, yy, f"Size {size} pt: The quick brown fox jumps over the lazy dog.",
                  size=size, role=f"size-{size}")
    text_line(c, rec, p, LEFT, 214, "Courier 10 pt: fixed-width 0123456789 iiii mmmm WWWW", font="Courier",
              role="monospace")
    s = "Go to page 2 (internal link with an explicit destination)."
    end = text_line(c, rec, p, LEFT, 196, s, font="Helvetica", size=9, color=(0, 0, 0.8), role="internal-link")
    rect = (LEFT, 193.0, end, 205.0)
    # reportlab writes this as /Dest [<page 2> /Fit] (an explicit destination).
    c.linkAbsolute("", "p2", Rect=rect, thickness=0)
    rec.add(kind="link", page=p, role="internal-link", rect=rect, dest="explicit /Dest [page 2 /Fit]")
    url = "https://creativecommons.org/publicdomain/zero/1.0/"
    s = "CC0 1.0 deed (URI link): " + url
    end = text_line(c, rec, p, LEFT, 184, s, font="Helvetica", size=9, color=(0, 0, 0.8), role="uri-link")
    rect = (LEFT, 181.0, end, 193.0)
    c.linkURL(url, rect, relative=0, thickness=0)
    rec.add(kind="link", page=p, role="uri-link", rect=rect, uri=url)

    # ---- rotated text along the right margin
    c.saveState()
    c.translate(575, 330)
    c.rotate(90)
    s = "Rotated 90 degrees counter-clockwise"
    t = c.beginText(0, 0)
    t.setFont("Helvetica", 9)
    t.textOut(s)
    c.drawText(t)
    c.restoreState()
    rec.add(kind="text", page=p, role="rotated-90", text=s, font="Helvetica", size=9.0, x=575.0, y=330.0,
            advance=stringWidth(s, "Helvetica", 9), color=(0, 0, 0),
            direction="baseline runs up (+y); set with cm rotation, not /Rotate")

    footer(c, rec, p, 2)


LEFT_COL = [
    ["Terminal viewers have to decide where every word",
     "of a page belongs on a coarse grid of character",
     "cells. A two-column page is the classic trap: if the",
     "text is read straight across the page, lines from",
     "the left and right columns interleave, and the result",
     "is nonsense. A reflow mode has to find the columns",
     "first and read the left one completely before it",
     "starts on the right one."],
    ["Hyphenation is the second trap. When a word is",
     "broken at the end of a line, as the word infor-",
     "mation is in this sentence, the two halves have to",
     "be joined again and the hyphen has to be dropped.",
     "A hyphen that belongs to the word itself, as in",
     "two-column, must be kept."],
    # This paragraph continues in the right column, so its last line here is
    # justified too.
    ["This column is justified: the space between words",
     "is stretched with the Tw operator so that every full",
     "line ends exactly at the right edge of the column.",
     "The last word of this column is split, and a cor-"],
]
RIGHT_COL = [
    ["rect reader joins its two halves across the column",
     "break. This column is set ragged right, so the word",
     "spacing is natural and each line simply ends where",
     "the next word would no longer fit."],
    ["Numbers and units deserve a test as well: 3.5 mm,",
     "12 pt, 0.5 pt rules and 72 points per inch."],
    ["Both columns use the standard Times-Roman font,",
     "which is not embedded, so a viewer has to fall back",
     "to built-in metrics for the glyph widths. The page",
     "is US Letter, 612 by 792 points."],
]


def page2(c, rec):
    p = 2
    c.bookmarkPage("p2")
    c.addOutlineEntry("Page 2: two-column text", "p2", level=0)
    text_line(c, rec, p, LEFT, 720, "Two-column layout", font="Helvetica-Bold", size=14, role="heading")
    text_line(c, rec, p, LEFT, 704,
              "Left column justified with Tw, right column ragged right; Times-Roman 10 pt on 12 pt leading.",
              font="Times-Italic", size=9, role="subtitle")
    c.bookmarkHorizontalAbsolute("p2-left", 692)
    c.addOutlineEntry("Left column (justified)", "p2-left", level=1)
    c.bookmarkHorizontal("p2-right", COL_X[1], 692)
    c.addOutlineEntry("Right column (ragged right)", "p2-right", level=1)
    for col, (x, paras) in enumerate(((COL_X[0], LEFT_COL), (COL_X[1], RIGHT_COL))):
        y = 680.0
        for pi, para in enumerate(paras):
            for li, s in enumerate(para):
                w = stringWidth(s, "Times-Roman", 10)
                if w > COL_W:
                    raise SystemExit(f"line too wide ({w:.1f} pt): {s}")
                last = li == len(para) - 1
                continues = col == 0 and pi == len(paras) - 1
                tw = 0.0
                if col == 0 and (not last or continues):
                    tw = (COL_W - w) / s.count(" ")
                role = "col-%s-p%d" % ("left" if col == 0 else "right", pi + 1)
                text_line(c, rec, p, x, y, s, tw=tw, role=role)
                y -= 12
            y -= 12  # blank line between paragraphs
    stroke_line(c, rec, p, LEFT, 100, 200, 100, 0.5, "footnote-rule")
    text_line(c, rec, p, LEFT, 88, "1 Footnotes use a smaller size (Times-Roman 8 pt) and sit below a short rule.",
              size=8, role="footnote")
    footer(c, rec, p, 2)


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", default=os.path.join(here, "synthetic.pdf"))
    ap.add_argument("--json", default=os.path.join(here, "synthetic.json"))
    a = ap.parse_args()

    register_ligature_font()
    rec = Recorder()
    c = canvas.Canvas(a.out, pagesize=letter, invariant=1)
    c.setTitle("sshpdf synthetic test document")
    c.setAuthor("sshpdf contributors")
    c.setSubject("Deterministic test page with known text, rules, shapes and an image")
    c.setKeywords("sshpdf, test corpus, CC0")
    c.setCreator("corpus/gen_synthetic.py")
    page1(c, rec)
    c.showPage()
    page2(c, rec)
    c.showPage()
    c.save()

    meta = {
        "file": os.path.basename(a.out),
        "generator": "corpus/gen_synthetic.py",
        "license": "CC0-1.0",
        "units": "PDF points; origin bottom-left; y up; text (x, y) = baseline start",
        "page_size": [PAGE_W, PAGE_H],
        "pages": 2,
        "notes": [
            "advance = advance width from the AFM metrics incl. Tc/Tw/Tz, not the ink box",
            "all fonts are standard-14 Type 1 fonts and are not embedded",
            "the ligature line uses /Times-Roman with /Differences [129 /fi 141 /fl] and no ToUnicode",
        ],
        "items": rec.items,
    }
    with open(a.json, "w", encoding="utf-8") as f:
        json.dump(meta, f, ensure_ascii=False, indent=1)
        f.write("\n")
    print(f"wrote {a.out} and {a.json} ({len(rec.items)} items)")


if __name__ == "__main__":
    main()
