#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Count the executables Ventoy commits, by looking at what the files are rather
# than at what they are called.
#
# # Why this exists
#
# Upstream ships `BLOB_List.md`, a hand-maintained table of the binaries it
# commits. It is a good-faith document and it is out of date, which is what
# happens to every hand-written list of a generated set. Deriving the same set
# by machine against Ventoy 1.1.07 finds a much larger number.
#
# # Why detection is by magic bytes and not by extension
#
# An earlier version of this script classified a file as a binary if its first
# bytes were ELF or PE magic, or if its *name* ended in `.xz`, `.gz`, `.zst` or
# `.lz4`. That produced 806, and 806 is not a number that means anything: it is
# 754 real executables plus 52 files that merely had a compression suffix,
# while ignoring hundreds of compressed executables whose names end in
# something else.
#
# The fix is to open the archives. A file is counted when its own first bytes
# are ELF or PE, or when decompressing it yields ELF or PE. That is the honest
# question, because `busybox64.xz` and `dm-mod.ko.xz` are executables that a
# running system unpacks and uses, and a reader has exactly as little ability
# to verify them as any loose binary.
#
# Against Ventoy 1.1.07 that gives:
#
#     1079 executables    754 loose, 325 compressed
#      182 paths named in BLOB_List.md
#       59 Linux kernel modules
#
# None of that is an accusation. It is the ordinary drift of a list a person
# updates by hand beside a set a build script generates, and it is why this
# project derives its inventory by machine on every run.
#
# # Output
#
# Writes `inventory.json`: every executable, whether the blob list names it, and
# what upstream says its origin is. That file feeds the manifest generator.

import argparse
import bz2
import collections
import gzip
import hashlib
import json
import lzma
import os
import re
import subprocess
import sys

# The first bytes that mean "this is a program", for the formats Ventoy ships.
EXECUTABLE_MAGIC = (b"\x7fELF", b"MZ")

# Compressed containers worth opening. Each maps to the module that reads it.
# zip is deliberately absent: the three in the tree are source archives rather
# than single compressed executables, and treating one as "an executable" would
# be a category error in the other direction.
COMPRESSORS = {
    b"\xfd7zXZ": lzma.open,
    b"\x1f\x8b": gzip.open,
    b"BZh": bz2.open,
}

# How far to read inside an archive. Four bytes is all a magic check needs, and
# reading more of a 30 MB kernel module several hundred times is a minute of CI
# spent learning nothing.
PEEK = 8


def tracked_files(root):
    """Every file git tracks, which is the set that reaches a user."""
    out = subprocess.run(
        ["git", "-C", root, "ls-files", "-z"],
        capture_output=True,
        check=True,
    )
    return [p for p in out.stdout.decode("utf-8").split("\0") if p]


def looks_executable(head):
    return any(head.startswith(m) for m in EXECUTABLE_MAGIC)


def classify(path):
    """Return 'loose', 'compressed' or None for one file.

    'compressed' means the file is an archive whose contents begin with
    executable magic. A corrupt or truncated archive returns None rather than
    raising: a file this script cannot read is a file it should not count, and
    a crash part way through an inventory is worse than a conservative answer.
    """
    try:
        with open(path, "rb") as handle:
            head = handle.read(PEEK)
    except OSError:
        return None

    if looks_executable(head):
        return "loose"

    for magic, opener in COMPRESSORS.items():
        if head.startswith(magic):
            try:
                with opener(path, "rb") as handle:
                    if looks_executable(handle.read(PEEK)):
                        return "compressed"
            except Exception:
                return None
            return None

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
    # `rowspan` lets a row inherit the previous row's source and recipe, so the
    # parser carries them forward rather than treating a short row as blank.
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
        # Two entries are spelled ISNTALL rather than INSTALL. Corrected and
        # flagged rather than silently rewritten: a path that does not exist
        # should stay visible.
        typo = clean.startswith("ISNTALL/")
        if typo:
            clean = "INSTALL/" + clean[len("ISNTALL/"):]
        entries[clean] = {"source": source, "recipe": recipe, "upstream_typo": typo}
    return entries


def describes(listed, rel):
    """Find the blob list entry for a path, allowing for a missing suffix.

    The list names several files without the compression suffix they actually
    carry, so `busybox32` in the table is `busybox32.xz` on disk. Matching both
    spellings is the difference between six entries looking missing and six
    entries being found.
    """
    if rel in listed:
        return listed[rel]
    for suffix in (".xz", ".gz", ".bz2", ".zst", ".lz4"):
        if rel.endswith(suffix) and rel[: -len(suffix)] in listed:
            return listed[rel[: -len(suffix)]]
    return None


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

        described = describes(listed, rel)
        records.append({
            "path": rel,
            "kind": kind,
            "size": os.path.getsize(full),
            "documented": described is not None,
            "origin": (described or {}).get("source"),
            "recipe": (described or {}).get("recipe"),
            **({"sha256": sha256(full)} if with_hashes else {}),
        })

    records.sort(key=lambda r: r["path"])

    documented = sum(1 for r in records if r["documented"])
    kinds = collections.Counter(r["kind"] for r in records)
    grub = sum(1 for r in records if r["path"].startswith("INSTALL/grub/"))
    kmods = sum(
        1 for r in records
        if re.search(r"\.ko(\.(xz|gz|bz2))?$", r["path"])
    )

    return {
        "counts": {
            "executables_in_tree": len(records),
            "loose": kinds["loose"],
            "compressed": kinds["compressed"],
            "paths_named_in_blob_list": len(listed),
            "named_and_present": documented,
            "present_but_undocumented": len(records) - documented,
            "grub2_modules": grub,
            "linux_kernel_modules": kmods,
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
        "--hashes", action="store_true",
        help="also record SHA-256 of every file, which is slower",
    )
    parser.add_argument(
        "--print-counts", action="store_true",
        help="print the summary and write nothing",
    )
    args = parser.parse_args()

    result = build(args.root, args.hashes)
    counts = result["counts"]

    if args.print_counts:
        for key, value in counts.items():
            print(f"{key.replace('_', ' '):<28} {value}")
        return 0

    with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(result, handle, indent=2, sort_keys=True)
        handle.write("\n")

    print(f"wrote {args.out}")
    for key, value in counts.items():
        print(f"  {key.replace('_', ' '):<28} {value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
