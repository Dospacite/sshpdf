#!/usr/bin/env bash
# Download the original source files of the sshpdf test corpus into DIR and
# check their SHA-256 against the values recorded when the corpus was built
# (2026-11-05). See corpus/SOURCES.md for licenses and attribution.
#
# Usage: scripts/fetch_sources.sh DIR
#
# Downloads are untrusted input: use a fresh, empty DIR. A hash mismatch is
# reported but does not stop the script (a server may regenerate a file).
# The Wikipedia PDF is rendered on demand from the CURRENT revision of the
# article, so its hash will not match; the corpus copy is revision 111190598.
# License of this script: CC0 1.0.
set -euo pipefail

dir=${1:?usage: fetch_sources.sh DIR}
mkdir -p "$dir"
ua="sshpdf-corpus-fetch/1.0 (test corpus; see corpus/SOURCES.md)"
status=0

fetch() { # name url sha256
    local name=$1 url=$2 want=$3 got
    curl -fsSL --retry 3 --max-time 300 -A "$ua" -o "$dir/$name" "$url"
    got=$(sha256sum "$dir/$name" | cut -d' ' -f1)
    if [[ $want == - ]]; then
        echo "fetched   $name (no fixed hash: rendered on demand)"
    elif [[ $got == "$want" ]]; then
        echo "ok        $name"
    else
        echo "MISMATCH  $name: got $got, want $want" >&2
        status=1
    fi
}

fetch arxiv-2308.00711v1.pdf https://arxiv.org/pdf/2308.00711v1 \
    f6f6e24d76586eaf17b93b04d0c114d73cbc96364ba7709ed7a5b855bf145921
fetch arxiv-2402.14026v3.pdf https://arxiv.org/pdf/2402.14026v3 \
    9451a95a336c10753ebed292b0da88580b4517fd87f417ef676424cb0a6f7bd8
fetch riscv-privileged-20250508.pdf \
    https://github.com/riscv/riscv-isa-manual/releases/download/20250508/riscv-privileged-20250508.pdf \
    d0228bbecc76943aaa5685e381e0263a8c750d18b1b0c452b086b0ddd15a4955
fetch Parsl-deRSE25.pdf "https://zenodo.org/records/14922563/files/Parsl-deRSE25.pdf?download=1" \
    a52ffaec6f59058ed9436424c7d00aee5ba90ad4e5f3c6327829d166afcce152
fetch hangar-ae-fact-sheet.pdf https://www.nasa.gov/wp-content/uploads/2023/10/hangar-ae-fact-sheet.pdf \
    a0d8d0f89d8da824fce2c2d4373b287c9f5cabd1e1a58a92de169fba00f9ad4f
fetch byrne-euclid-1847-n38.jpg https://archive.org/download/firstsixbooksel00byrn/page/n38.jpg \
    d2515af833dd568a7ed60c74b6613c66d016213fb924dab9477362d00c67c251
# corpus copy: sha256 57c947a486b641716a561674af805e16725c6f11f8dd270d4a32790e309d9cad
fetch jawiki-kaibun.pdf "https://ja.wikipedia.org/api/rest_v1/page/pdf/%E5%9B%9E%E6%96%87" -

exit $status
