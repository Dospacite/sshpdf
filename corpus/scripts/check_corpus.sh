#!/usr/bin/env bash
# Summarize every PDF in the corpus with poppler-utils (pdfinfo, pdftotext,
# pdffonts, pdfimages): pages, size, encryption, /Rotate, extracted text length,
# font types and image count. encrypted.pdf is opened with user password
# "sshpdf". Exits non-zero if a file fails to open, if scanned.pdf has any
# text, or if another file has none.
#
# Usage: scripts/check_corpus.sh [CORPUS_DIR]   (default: the corpus directory)
# License of this script: CC0 1.0.
set -uo pipefail

dir=${1:-$(cd "$(dirname "$0")/.." && pwd)}
status=0
printf '%-26s %5s %9s  %-26s %-12s %8s %4s  %s\n' file pages bytes encryption rotate text imgs fonts
for f in "$dir"/*.pdf; do
    name=$(basename "$f")
    pw=()
    [[ $name == encrypted.pdf ]] && pw=(-upw sshpdf)
    if ! info=$(pdfinfo "${pw[@]}" "$f" 2>/dev/null); then
        echo "$name: pdfinfo FAILED" >&2
        status=1
        continue
    fi
    pages=$(awk -F': *' '/^Pages:/{print $2}' <<<"$info")
    enc=$(awk -F': *' '/^Encrypted:/{print $2}' <<<"$info" | sed 's/ (.*//')
    algo=$(grep -o 'algorithm:[^) ]*' <<<"$info" | head -1 | cut -d: -f2)
    copy=$(grep -o 'copy:[a-z]*' <<<"$info" | head -1)
    [[ $enc == yes ]] && enc="yes ${algo:-} ${copy:-}"
    rot=$(pdfinfo "${pw[@]}" -f 1 -l "$pages" "$f" 2>/dev/null | awk '/^Page +[0-9]+ rot:/{r = r $4 ","} END{print r}' | sed 's/,$//')
    [[ $rot =~ ^(0,)*0$ ]] && rot=0
    chars=$(pdftotext "${pw[@]}" -q "$f" - 2>/dev/null | tr -d '[:space:]' | wc -m)
    fonts=$(pdffonts "${pw[@]}" "$f" 2>/dev/null | awk 'NR>2{
            t=""; for (i=2;i<=NF;i++) if ($i ~ /^(Type|TrueType|CID|1C|3|1|0C)/) t=t (t?" ":"") $i; else if (t) break;
            print t}' | sort | uniq -c | awk '{c=$1; $1=""; printf "%s%s x%d", (n++?", ":""), substr($0,2), c}')
    imgs=$(pdfimages "${pw[@]}" -list "$f" 2>/dev/null | awk 'NR>2 && $3=="image"' | wc -l)
    printf '%-26s %5s %9s  %-26s %-12s %8s %4s  %s\n' "$name" "$pages" "$(stat -c %s "$f")" "$enc" "$rot" "$chars" "$imgs" "${fonts:-none}"
    if [[ $name == scanned.pdf ]]; then
        (( chars == 0 )) || { echo "scanned.pdf has a text layer" >&2; status=1; }
    else
        (( chars > 0 )) || { echo "$name: no text extracted" >&2; status=1; }
    fi
done
exit $status
