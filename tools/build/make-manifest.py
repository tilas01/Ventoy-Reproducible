#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Compare two builds, compare both against what upstream shipped, and write the
# one document a reader has to check.
#
# # Three questions, kept apart
#
# There are three different things a person might mean by "did it work", and
# merging them is how a reproducibility report becomes marketing:
#
#   1. Did our two builds agree with each other?     -> verdict
#   2. Did our build match the committed blob?       -> matches_upstream
#   3. Did we build it at all?                       -> origin
#
# A file can reproduce perfectly and still differ from upstream's copy, which is
# the interesting case and the one a summary that says "182/182" would bury. All
# three are recorded per file and counted separately.

import argparse
import hashlib
import json
import os
import sys


def sha256_and_blake3(path):
    """Both digests in one pass, streamed."""
    sha = hashlib.sha256()
    # BLAKE3 is not in the standard library. Where it is unavailable the field
    # is left empty rather than filled with a different algorithm's output,
    # because a manifest that lies about which hash it used is worse than one
    # with a gap.
    try:
        import blake3 as _blake3
        b3 = _blake3.blake3()
    except ImportError:
        b3 = None

    size = 0
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            sha.update(chunk)
            if b3 is not None:
                b3.update(chunk)
            size += len(chunk)

    return sha.hexdigest(), (b3.hexdigest() if b3 is not None else ""), size


def walk(root):
    """Every regular file under root, as repository-relative paths."""
    found = {}
    if not os.path.isdir(root):
        return found
    for base, _dirs, files in os.walk(root):
        for name in files:
            full = os.path.join(base, name)
            rel = os.path.relpath(full, root).replace(os.sep, "/")
            found[rel] = full
    return found


def load_reasons(path):
    """Read a not-built.tsv into {path: reason}."""
    reasons = {}
    if not os.path.exists(path):
        return reasons
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            parts = line.rstrip("\n").split("\t", 1)
            if len(parts) == 2:
                reasons[parts[0]] = parts[1]
    return reasons


def main():
    parser = argparse.ArgumentParser(description="Compare two builds and write a manifest.")
    parser.add_argument("--pass-one", required=True)
    parser.add_argument("--pass-two", required=True)
    parser.add_argument("--reference", required=True, help="the repository, for committed blobs")
    parser.add_argument("--ventoy-version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--upstream-commit", default=None)
    parser.add_argument("--source-date-epoch", type=int, required=True)
    parser.add_argument("--builder-image", required=True)
    parser.add_argument("--workflow-run", default=None)
    parser.add_argument("--out", default="manifest.json")
    parser.add_argument("--report", default="reproducibility-report.md")
    parser.add_argument("--summary", default=None, help="also append the report here")
    args = parser.parse_args()

    one = walk(args.pass_one)
    two = walk(args.pass_two)

    not_built = load_reasons(os.path.join(os.path.dirname(args.pass_one), "not-built.tsv"))

    entries = []
    reproduced = differed = only_one_pass = 0
    matched_upstream = differed_upstream = 0

    for rel in sorted(set(one) | set(two)):
        note = None

        if rel in one and rel in two:
            sha_a, b3_a, size_a = sha256_and_blake3(one[rel])
            sha_b, _b3_b, _size_b = sha256_and_blake3(two[rel])
            if sha_a == sha_b:
                verdict = "reproduced"
                reproduced += 1
            else:
                verdict = "differs"
                differed += 1
                note = "the two builds of this file produced different bytes"
        else:
            # Present in one pass and not the other. That is a build that is not
            # merely unreproducible but non-deterministic in what it emits, and
            # it is worth its own wording.
            present = one if rel in one else two
            sha_a, b3_a, size_a = sha256_and_blake3(present[rel])
            verdict = "unknown"
            only_one_pass += 1
            note = "produced by only one of the two builds"

        # Now the separate question: does our build match what upstream shipped?
        committed = os.path.join(args.reference, rel)
        matches_upstream = None
        if os.path.isfile(committed):
            sha_ref, _b3_ref, _size_ref = sha256_and_blake3(committed)
            matches_upstream = sha_ref == sha_a
            if matches_upstream:
                matched_upstream += 1
            else:
                differed_upstream += 1
                extra = (
                    "our build of this file does not match the binary upstream "
                    "committed; this is a fact to investigate, not proof of "
                    "wrongdoing, and the usual cause is a toolchain difference"
                )
                note = f"{note}; {extra}" if note else extra

        entry = {
            "path": rel,
            "size": size_a,
            "sha256": sha_a,
            "blake3": b3_a,
            "origin": "built",
            "verdict": verdict,
        }
        if matches_upstream is not None:
            entry["matches_upstream"] = matches_upstream
        if note:
            entry["note"] = note
        entries.append(entry)

    # Everything named as not built gets an entry too. A manifest that simply
    # omits what it could not do reads as a clean sweep, which is the single
    # most misleading thing this file could be.
    for rel, reason in sorted(not_built.items()):
        origin = (
            "upstream-binary"
            if "cannot be built here" in reason
            else "not-built"
        )
        entries.append({
            "path": rel,
            "size": 0,
            "sha256": "0" * 64,
            "blake3": "0" * 64,
            "origin": origin,
            "verdict": "unknown",
            "note": reason,
        })

    manifest = {
        "schema": 1,
        "provenance": {
            "ventoy_version": args.ventoy_version,
            "commit": args.commit,
            "source_date_epoch": args.source_date_epoch,
            "builder_image": args.builder_image,
        },
        "entries": entries,
    }
    if args.upstream_commit:
        manifest["provenance"]["upstream_commit"] = args.upstream_commit
    if args.workflow_run:
        manifest["provenance"]["workflow_run"] = args.workflow_run

    with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(manifest, handle, indent=2, sort_keys=False)
        handle.write("\n")

    report = [
        "# Reproducibility report",
        "",
        f"Upstream version `{args.ventoy_version}`, commit `{args.commit[:12]}`.",
        f"Built in `{args.builder_image}`.",
        "",
        "## Did our two builds agree with each other",
        "",
        f"- **{reproduced}** files reproduced byte for byte",
        f"- **{differed}** files differed between the two builds",
        f"- **{only_one_pass}** files appeared in only one of the two builds",
        "",
        "## Did our build match the binary upstream committed",
        "",
        f"- **{matched_upstream}** matched upstream's committed copy",
        f"- **{differed_upstream}** did not match upstream's committed copy",
        "",
        "A file that does not match upstream is not evidence of wrongdoing. The",
        "usual cause is a different compiler version or a different build host,",
        "and closing that gap is ordinary work. It is published because a gap",
        "nobody is told about is a gap nobody closes.",
        "",
        "## What was not built",
        "",
        f"**{len(not_built)}** entries were not built, each for a stated reason:",
        "",
    ]
    for rel, reason in sorted(not_built.items()):
        report.append(f"- `{rel}`: {reason}")
    report.append("")

    text = "\n".join(report)
    with open(args.report, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)

    if args.summary:
        with open(args.summary, "a", encoding="utf-8", newline="\n") as handle:
            handle.write(text)

    # Outputs the workflow reads back.
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with open(github_output, "a", encoding="utf-8", newline="\n") as handle:
            handle.write(f"reproduced={reproduced}\n")
            handle.write(f"differed={differed + only_one_pass}\n")
            handle.write(f"matched_upstream={matched_upstream}\n")
            handle.write(f"differed_upstream={differed_upstream}\n")

    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
