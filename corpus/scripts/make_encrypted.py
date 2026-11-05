#!/usr/bin/env python3
"""Write an encrypted copy of a PDF (PDF Standard security handler).

Usage:
  make_encrypted.py SRC OUT --user-password PW [--owner-password PW]
                    [--algorithm AES-256|AES-128|RC4-128|RC4-40]
                    [--deny print,modify,copy,annotate,forms,accessibility,assemble,print-hq]

An empty --user-password ("") makes an "owner password only" file: it opens
without a prompt, but its strings and streams are still encrypted and the
permission flags (/P) apply.

The whole document is cloned (pages, outline, names, metadata), then encrypted.
AES-256 is /V 5 /R 6; AES-128 is /V 4 /R 4 with /CFM /AESV2. The output is NOT
byte-for-byte reproducible (random salts and IVs). Requires pypdf (tested with
6.21.4) and cryptography. License of this script: CC0 1.0.
"""

import argparse

from pypdf import PdfReader, PdfWriter
from pypdf.constants import UserAccessPermissions as UAP
from pypdf.generic import NameObject, NumberObject

DENY = {
    "print": UAP.PRINT,
    "modify": UAP.MODIFY,
    "copy": UAP.EXTRACT,
    "annotate": UAP.ADD_OR_MODIFY,
    "forms": UAP.FILL_FORM_FIELDS,
    "accessibility": UAP.EXTRACT_TEXT_AND_GRAPHICS,
    "assemble": UAP.ASSEMBLE_DOC,
    "print-hq": UAP.PRINT_TO_REPRESENTATION,
}


def main():
    ap = argparse.ArgumentParser(description="Write an encrypted copy of a PDF.")
    ap.add_argument("src")
    ap.add_argument("out")
    ap.add_argument("--user-password", required=True)
    ap.add_argument("--owner-password", default=None)
    ap.add_argument("--algorithm", default="AES-256")
    ap.add_argument("--deny", default="", help="comma-separated permissions to withhold")
    a = ap.parse_args()

    perms = UAP.all()
    for name in filter(None, (s.strip() for s in a.deny.split(","))):
        perms &= ~DENY[name]

    writer = PdfWriter(clone_from=PdfReader(a.src))
    writer.encrypt(
        user_password=a.user_password,
        owner_password=a.owner_password,
        permissions_flag=perms,
        algorithm=a.algorithm,
    )
    # pypdf writes /P unsigned (4294967292); PDF integers are 32-bit signed and
    # other writers use the signed form (-4). Key derivation only uses the low
    # 4 bytes of P, so swapping in the signed value keeps the file consistent.
    p = int(perms) & 0xFFFFFFFF
    p = p - (1 << 32) if p >= (1 << 31) else p
    writer._encrypt_entry[NameObject("/P")] = NumberObject(p)
    with open(a.out, "wb") as f:
        writer.write(f)
    print(f"{a.out}: {a.algorithm}, user password {a.user_password!r}, /P {p}")


if __name__ == "__main__":
    main()
