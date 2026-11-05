#!/usr/bin/env python3
"""Copy selected pages of a PDF into a new PDF.

Usage: extract_pages.py SRC OUT PAGES
  PAGES is a 1-based page list such as "17,18,20-24,31,32,36".

Uses pypdf's PdfWriter.append(), which keeps link annotations, named
destinations and outline (bookmark) entries whose targets are among the kept
pages, and drops the ones that would dangle. The page content, fonts and
images are copied unchanged. The document info (title, author, ...) is copied
and the Producer gets a note about the extraction.

Requires pypdf (tested with 6.21.4). The output is deterministic for a given
pypdf version. License of this script: CC0 1.0.
"""

import argparse

from pypdf import PdfReader, PdfWriter
from pypdf import __version__ as pypdf_version


def parse_pages(spec, count):
    pages = []
    for part in spec.split(","):
        part = part.strip()
        if "-" in part:
            a, b = part.split("-")
            pages.extend(range(int(a), int(b) + 1))
        else:
            pages.append(int(part))
    for p in pages:
        if not 1 <= p <= count:
            raise SystemExit(f"page {p} is out of range 1..{count}")
    return [p - 1 for p in pages]


def main():
    ap = argparse.ArgumentParser(description="Copy selected pages of a PDF into a new PDF.")
    ap.add_argument("src")
    ap.add_argument("out")
    ap.add_argument("pages", help='1-based list, e.g. "1,3-5,9"')
    a = ap.parse_args()

    reader = PdfReader(a.src)
    pages = parse_pages(a.pages, len(reader.pages))
    writer = PdfWriter()
    writer.append(reader, pages=pages)

    info = {}
    for key, value in (reader.metadata or {}).items():
        value = value.get_object() if hasattr(value, "get_object") else value
        info[key] = str(value)
    producer = info.get("/Producer", "").strip()
    note = f"pages {a.pages} of {len(reader.pages)} extracted with pypdf {pypdf_version}"
    info["/Producer"] = f"{producer}; {note}" if producer else note
    writer.add_metadata(info)
    writer.compress_identical_objects(remove_duplicates=True, remove_unreferenced=True)

    with open(a.out, "wb") as f:
        writer.write(f)
    print(f"{a.out}: {len(pages)} pages from {a.src}")


if __name__ == "__main__":
    main()
