#!/usr/bin/env python3
"""Copy pages of a PDF and give some of them a /Rotate attribute.

Usage: make_rotated.py SRC OUT PAGE:ANGLE [PAGE:ANGLE ...]
  e.g. make_rotated.py paper-2col.pdf rotated.pdf 1:0 2:90 3:180 4:270

PAGE is 1-based in SRC; output pages follow the argument order. ANGLE is 0, 90,
180 or 270 (clockwise, as /Rotate is defined). Angle 0 leaves the page without
a /Rotate key. Only the /Rotate entry changes; content streams are untouched,
so text, link rectangles and paths stay in unrotated page space.

Requires pypdf (tested with 6.21.4); output is deterministic for a given pypdf
version. License of this script: CC0 1.0.
"""

import argparse

from pypdf import PdfReader, PdfWriter
from pypdf import __version__ as pypdf_version
from pypdf.generic import NameObject, NumberObject


def main():
    ap = argparse.ArgumentParser(description="Copy pages and set /Rotate on them.")
    ap.add_argument("src")
    ap.add_argument("out")
    ap.add_argument("specs", nargs="+", metavar="PAGE:ANGLE")
    a = ap.parse_args()

    reader = PdfReader(a.src)
    plan = []
    for spec in a.specs:
        page, angle = (int(v) for v in spec.split(":"))
        if angle not in (0, 90, 180, 270):
            raise SystemExit(f"bad angle in {spec}")
        if not 1 <= page <= len(reader.pages):
            raise SystemExit(f"page {page} out of range")
        if reader.pages[page - 1].get("/Rotate"):
            raise SystemExit(f"source page {page} already has /Rotate")
        plan.append((page, angle))

    writer = PdfWriter()
    writer.append(reader, pages=[p - 1 for p, _ in plan])
    for out_page, (_src, angle) in zip(writer.pages, plan):
        if angle:
            out_page[NameObject("/Rotate")] = NumberObject(angle)

    info = {}
    for key, value in (reader.metadata or {}).items():
        value = value.get_object() if hasattr(value, "get_object") else value
        info[key] = str(value)
    note = "pages:rotation " + " ".join(a.specs) + f" set with pypdf {pypdf_version}"
    producer = info.get("/Producer", "").strip()
    info["/Producer"] = f"{producer}; {note}" if producer else note
    writer.add_metadata(info)
    writer.compress_identical_objects(remove_duplicates=True, remove_unreferenced=True)
    with open(a.out, "wb") as f:
        writer.write(f)
    print(f"{a.out}: " + ", ".join(f"page {i + 1} <- src p{s} /Rotate {r}" for i, (s, r) in enumerate(plan)))


if __name__ == "__main__":
    main()
