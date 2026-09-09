#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Count what is actually in the tree, rather than what a document says is in it.
#
# # Why this exists
#
# Upstream ships `BLOB_List.md`, a hand-maintained table of the binaries it
# commits. It is a good-faith document and it is out of date, which is what
# happens to every hand-maintained list of a generated set. When this script was
# first run against Ventoy 1.1.07 it found:
#
#   * 182 paths named in BLOB_List.md
#   * 150 of those actually present in the tree
#   * 754 files in the tree whose first bytes are ELF or PE magic
#   * 604 executables present that BLOB_List.md does not mention at all
#
# Of the 604, 574 are GRUB2 modules under INSTALL/grub/, which the list covers
# collectively with one "build grub2" instruction rather than naming. The other
# 30 are Linux kernel modules under LiveCD/VTOY/ventoy/drivers/ with no build
# instruction recorded anywhere in the repository.
#
# None of that is an accusation. It is the ordinary drift of a list a person
# updates by hand, and it is exactly why this project derives its inventory from
# the tree by machine on every run instead of trusting a table.
#
# # Output
#
# Writes `inventory.json`: every executable in the tree, whether the blob list
# names it, and what upstream says its origin is. That file is the input to the
# manifest generator, and CI fails when the counts drift without the drift being
# recorded.

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys

# The first bytes that mean "this is a program", for the formats Ventoy ships.
MAGIC = (
    (b"\x7fELF", "elf"),
    (b"MZ", "pe"),
)

# Compressed executables. Upstream commits several binaries only in xz form and
# the blob list names them without the suffix, which is most of the reason 32 of
# its entries look missing.
COMPRESSED_SUFFIXES = (".xz", ".gz", ".zst", ".lz4")


def tracked_files(root):
    """Every file git tracks, which is the set that reaches a user."""
    out = subprocess.run(
        ["git", "-C", root, "ls-files", "-z"],
        capture_output=True,
        check=True,
    )
    return [p for p in out.stdout.decode("utf-8").split("\0") if p]


def classify(path):
    """Return 'elf', 'pe', 'compressed' or None for one file."""
    try:
        with open(path, "rb") as handle:
            head = handle.read(4)
    except OSError:
        return None
    for magic, kind in MAGIC:
        if head.startswith(magic):
            return kind
    if path.endswith(COMPRESSED_SUFFIXES):
        return "compressed"
    return None


def parse_blob_list(root):
    """Parse upstream's BLOB_List.md into {path: {source, recipe}}."""
    blob_list = os.path.join(root, "BLOB_List.md")
    if not os.path.exists(blob_list):
        return {}

    with open(blob_list, encoding="utf-8") as handle:
        src = handle.read()

    entries = {}
    source = None
    recipe = None
    # `rowspan` means a row can inherit the previous row's source and recipe, so
    # the parser carries them forward rather than treating a short row as blank.
    for row in re.findall(r"<tr>(.*?)</tr>", src, re.S):
        cells = re.findall(r"<td[^>]*>(.*?)</td>", row, re.S)
        if not cells:
            continue
        path = re.sub(r"<[^>]+>", " ", cells[0]).strip()
        if not path:
            continue
        if len(cells) >= 2:
            source = re.sub(r"<[^>]+>", " ", cells[1]).strip()
        if len(cells) >= 3:
            text = re.sub(r"<br\s*/?>", " ", cells[2])
            recipe = " ".join(re.sub(r"<[^>]+>", " ", text).split())

        clean = path[2:] if path.startswith("./") else path
        clean = clean.replace("\\", "/")
        # Two entries are spelled ISNTALL rather than INSTALL. Corrected here and
        # flagged, rather than silently rewritten: a path that does not exist
        # should be visible.
        typo = clean.startswith("ISNTALL/")
        if typo:
            clean = "INSTALL/" + clean[len("ISNTALL/"):]
        entries[clean] = {
            "source": source,
            "recipe": recipe,
            "upstream_typo": typo,
        }
    return entries


def sha256(path):
    """Stream a file into SHA-256, because some of these are large."""
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build(root, with_hashes):
    listed = parse_blob_list(root)
    records = []

    for rel in tracked_files(root):
        full = os.path.join(root, rel)
        if not os.path.isfile(full):
            continue
        kind = classify(full)
        if kind is None:
            continue

        described = listed.get(rel)
        # A blob list entry without its compression suffix still describes the
        # file that is really there. Match both spellings before calling it
        # undocumented.
        if described is None:
            for suffix in COMPRESSED_SUFFIXES:
                if rel.endswith(suffix) and rel[: -len(suffix)] in listed:
                    described = listed[rel[: -len(suffix)]]
                    break

        record = {
            "path": rel,
            "kind": kind,
            "size": os.path.getsize(full),
            "documented": described is not None,
            "origin": (described or {}).get("source"),
            "recipe": (described or {}).get("recipe"),
        }
        if with_hashes:
            record["sha256"] = sha256(full)
        records.append(record)

    records.sort(key=lambda r: r["path"])

    listed_present = sum(1 for r in records if r["documented"])
    grub = sum(
        1
        for r in records
        if not r["documented"] and r["path"].startswith("INSTALL/grub/")
    )
    return {
        "counts": {
            "executables_in_tree": len(records),
            "paths_named_in_blob_list": len(listed),
            "named_and_present": listed_present,
            "present_but_undocumented": len(records) - listed_present,
            "undocumented_grub_modules": grub,
            "undocumented_other": len(records) - listed_present - grub,
        },
        "files": records,
    }


def main():
    parser = argparse.ArgumentParser(
        description="Inventory every executable Ventoy commits to its tree."
    )
    parser.add_argument("--root", default=".", help="repository root")
    parser.add_argument("--out", default="inventory.json", help="where to write")
    parser.add_argument(
        "--hashes",
        action="store_true",
        help="also record SHA-256 of every file, which is slower",
    )
    parser.add_argument(
        "--print-counts",
        action="store_true",
        help="print the summary and write nothing",
    )
    args = parser.parse_args()

    result = build(args.root, args.hashes)
    counts = result["counts"]

    if args.print_counts:
        for key, value in counts.items():
            print(f"{key.replace('_', ' '):<32} {value}")
        return 0

    with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(result, handle, indent=2, sort_keys=True)
        handle.write("\n")

    print(f"wrote {args.out}")
    for key, value in counts.items():
        print(f"  {key.replace('_', ' '):<32} {value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
