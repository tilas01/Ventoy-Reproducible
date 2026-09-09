#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Fetch the compilers Ventoy is built with, and refuse to proceed if any of them
# is not the one we pinned.
#
# # Why this file is the most important one in the build
#
# Upstream's own CI begins by downloading seven tarballs from a GitHub release
# called `vtoytoolchain`, with no hash, no signature and no verification of any
# kind. Those tarballs are compilers. A compiler you did not check is a strictly
# worse problem than a binary you did not check, because it silently affects
# every binary built afterwards, and Ken Thompson explained why in 1984.
#
# This script does not fix that. Nothing here builds GCC from source, so the
# compiler remains a binary somebody else produced. What it does is remove the
# part where the compiler can change without anybody noticing: every tarball is
# checked against `toolchains.lock`, and a mismatch stops the build.
#
# The honest summary, which belongs in the documentation and not only here:
# pinning tells you *which* unverified compiler you got. It does not tell you
# the compiler is honest.
#
# # Usage
#
#     bash tools/build/fetch-toolchains.sh toolchains/cache

set -euo pipefail

DEST="${1:-toolchains/cache}"
LOCK="${LOCK:-tools/build/toolchains.lock}"

if [ ! -f "$LOCK" ]; then
    echo "error: $LOCK does not exist; there is nothing to check against" >&2
    exit 2
fi

mkdir -p "$DEST"

fail=0
checked=0

# The lock is `sha256  url` per line, the same shape `sha256sum` writes, so it
# can be eyeballed and regenerated with ordinary tools.
while read -r expected url; do
    case "$expected" in
        ''|'#'*) continue ;;
    esac

    name="$(basename "${url%%\?*}")"
    target="$DEST/$name"

    if [ -f "$target" ]; then
        echo "cached:   $name"
    else
        echo "fetching: $name"
        # `--fail` so an HTML error page does not become a tarball, and a retry
        # because a release CDN times out often enough to matter in cron.
        curl --fail --location --silent --show-error \
             --retry 3 --retry-delay 5 \
             --output "$target.part" "$url"
        mv "$target.part" "$target"
    fi

    actual="$(sha256sum "$target" | cut -d' ' -f1)"
    checked=$((checked + 1))

    if [ "$actual" != "$expected" ]; then
        echo "" >&2
        echo "MISMATCH on $name" >&2
        echo "  expected $expected" >&2
        echo "  got      $actual" >&2
        echo "  from     $url" >&2
        echo "" >&2
        echo "This means the file at that URL is not the file this project was" >&2
        echo "pinned to. It may be a re-upload, or it may not. Nothing is built" >&2
        echo "until a person decides which, and updates $LOCK deliberately." >&2
        # The bad file is removed so that a re-run does not treat it as cached
        # and reach a different conclusion the second time.
        rm -f "$target"
        fail=1
    else
        echo "verified: $name"
    fi
done < "$LOCK"

if [ "$checked" -eq 0 ]; then
    echo "error: $LOCK contained no entries" >&2
    exit 2
fi

if [ "$fail" -ne 0 ]; then
    echo "" >&2
    echo "refusing to build with unverified toolchains" >&2
    exit 1
fi

echo ""
echo "all $checked toolchains match $LOCK"
