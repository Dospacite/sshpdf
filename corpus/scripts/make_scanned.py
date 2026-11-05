#!/usr/bin/env python3
"""Wrap a JPEG scan into a one-page, image-only PDF (no text layer, no fonts).

Usage: make_scanned.py SRC.jpg OUT.pdf [--dpi 300] [--title T] [--author A] [--subject S]

The PDF is written by hand, like typical scanner output: one page whose content
stream is just "q W 0 0 H 0 0 cm /Im0 Do Q", and one /DCTDecode image XObject
holding the JPEG bytes unchanged (nothing is re-encoded). There are no font
resources and no text operators. The page size is the image size at --dpi
(2828 x 3777 px at 300 dpi -> 678.72 x 906.48 pt). The file /ID is the MD5 of the
JPEG, so the output is byte-for-byte deterministic.

Requires Pillow (only to read the JPEG size and mode). License of this script: CC0 1.0.
"""

import argparse
import hashlib

from PIL import Image

COLORSPACES = {"L": "/DeviceGray", "RGB": "/DeviceRGB", "CMYK": "/DeviceCMYK"}


def num(v):
    s = f"{v:.4f}".rstrip("0").rstrip(".")
    return s or "0"


def pdf_string(s):
    if s.isascii():
        return "(" + s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)") + ")"
    return "<FEFF" + s.encode("utf-16-be").hex().upper() + ">"


def main():
    ap = argparse.ArgumentParser(description="Wrap a JPEG into an image-only PDF page.")
    ap.add_argument("src")
    ap.add_argument("out")
    ap.add_argument("--dpi", type=float, default=300.0)
    ap.add_argument("--title", default="")
    ap.add_argument("--author", default="")
    ap.add_argument("--subject", default="")
    a = ap.parse_args()

    with open(a.src, "rb") as f:
        jpeg = f.read()
    with Image.open(a.src) as im:
        if im.format != "JPEG":
            raise SystemExit(f"{a.src} is {im.format}, expected JPEG")
        px_w, px_h = im.size
        mode = im.mode
    if mode not in COLORSPACES:
        raise SystemExit(f"unsupported JPEG mode {mode}")
    if mode == "CMYK" and b"Adobe" in jpeg[:4096]:
        raise SystemExit("Adobe CMYK JPEGs need a /Decode array; not handled here")
    w_pt = px_w * 72.0 / a.dpi
    h_pt = px_h * 72.0 / a.dpi

    content = f"q {num(w_pt)} 0 0 {num(h_pt)} 0 0 cm /Im0 Do Q\n".encode()
    info = ["/Creator (corpus/scripts/make_scanned.py)", "/Producer (corpus/scripts/make_scanned.py)"]
    for key, value in (("Title", a.title), ("Author", a.author), ("Subject", a.subject)):
        if value:
            info.append(f"/{key} {pdf_string(value)}")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {num(w_pt)} {num(h_pt)}] "
         f"/Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>").encode(),
        (f"<< /Type /XObject /Subtype /Image /Width {px_w} /Height {px_h} "
         f"/ColorSpace {COLORSPACES[mode]} /BitsPerComponent 8 /Filter /DCTDecode "
         f"/Length {len(jpeg)} >>\nstream\n").encode() + jpeg + b"\nendstream",
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"endstream",
        ("<< " + " ".join(info) + " >>").encode(),
    ]
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    file_id = hashlib.md5(jpeg).hexdigest().upper()
    out += (f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R /Info 6 0 R "
            f"/ID [<{file_id}> <{file_id}>] >>\nstartxref\n{xref}\n%%EOF\n").encode()
    with open(a.out, "wb") as f:
        f.write(out)
    print(f"{a.out}: {px_w}x{px_h} px {mode} at {a.dpi:g} dpi -> {num(w_pt)} x {num(h_pt)} pt")


if __name__ == "__main__":
    main()
