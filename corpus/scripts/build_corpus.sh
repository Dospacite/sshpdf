#!/usr/bin/env bash
# Rebuild the sshpdf test corpus from the downloaded sources.
#
# Usage: scripts/build_corpus.sh SRC_DIR [OUT_DIR]
#   SRC_DIR  files from scripts/fetch_sources.sh
#   OUT_DIR  defaults to the corpus directory (the parent of this script)
#
# Needs python3 with reportlab, pypdf, cryptography and Pillow, e.g.
#   python3 -m venv venv && venv/bin/pip install reportlab pypdf cryptography pillow
#   PYTHON=venv/bin/python scripts/build_corpus.sh SRC_DIR
# Tested with Python 3.14, reportlab 5.1.0, pypdf 6.21.4, cryptography 50.0.2.
# Everything except encrypted*.pdf (random salts/IVs) and cjk.pdf (rendered on
# demand) rebuilds byte-for-byte. License of this script: CC0 1.0.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
src=${1:?usage: build_corpus.sh SRC_DIR [OUT_DIR]}
out=${2:-$(dirname "$here")}
py=${PYTHON:-python3}
run() { "$py" -I "$@"; }

cp "$src/arxiv-2308.00711v1.pdf" "$out/paper-2col.pdf"
cp "$src/arxiv-2402.14026v3.pdf" "$out/outline-links.pdf"
cp "$src/hangar-ae-fact-sheet.pdf" "$out/brochure-photos.pdf"
cp "$src/jawiki-kaibun.pdf" "$out/cjk.pdf"

run "$here/extract_pages.py" "$src/riscv-privileged-20250508.pdf" "$out/manual-tables.pdf" \
    17,18,20,21,22,23,24,31,32,36
run "$here/extract_pages.py" "$src/Parsl-deRSE25.pdf" "$out/slides.pdf" \
    1,3,4,5,7,8,9,11,12,16

run "$here/make_scanned.py" "$src/byrne-euclid-1847-n38.jpg" "$out/scanned.pdf" --dpi 300 \
    --title "The First Six Books of the Elements of Euclid (Byrne, 1847), Book I, Proposition I (page scan)" \
    --author "Oliver Byrne (1847); scan: Smithsonian Libraries via Internet Archive" \
    --subject "Image-only page, no text layer. Source: archive.org/details/firstsixbooksel00byrn, leaf n38"

run "$here/../gen_synthetic.py" --out "$out/synthetic.pdf" --json "$out/synthetic.json"

run "$here/make_encrypted.py" "$out/outline-links.pdf" "$out/encrypted.pdf" \
    --user-password sshpdf --owner-password sshpdf-owner --algorithm AES-256
run "$here/make_encrypted.py" "$out/synthetic.pdf" "$out/encrypted-owner-only.pdf" \
    --user-password "" --owner-password sshpdf-owner --algorithm AES-128 --deny modify,copy,annotate

run "$here/make_rotated.py" "$out/paper-2col.pdf" "$out/rotated.pdf" 1:0 2:90 4:270 5:180
