#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Unpack the pinned toolchains where Ventoy's build scripts expect to find them,
# and build the two libraries those scripts assume already exist.
#
# Sourced by `build-blobs.sh`, not run on its own, because it exports PATH.
#
# # What upstream's scripts expect
#
# `INSTALL/all_in_one.sh` puts four directories on PATH and every component's
# `build.sh` then calls compilers by bare name:
#
#     /opt/gcc-linaro-7.4.1-2019.02-x86_64_aarch64-linux-gnu/bin
#     /opt/aarch64--uclibc--stable-2020.08-1/bin
#     /opt/mips-loongson-gcc7.3-linux-gnu/2019.06-29/bin
#     /opt/mips64el-linux-musl-gcc730/bin
#
# and `/opt/diet32`, `/opt/diet64` for dietlibc.
#
# # The fourth one does not exist
#
# `/opt/mips64el-linux-musl-gcc730` is on that PATH and is not among the seven
# archives upstream's own CI downloads. The musl tarball it does download is
# musl *source*, which is not the same thing: turning it into a mips64el cross
# compiler is a full crosstool build that nothing in the repository performs.
#
# So `mips64el-linux-musl-gcc` is unavailable here, and every component that
# calls it cannot be built. That is recorded by name in the manifest rather than
# worked around, because a build that silently skips a target produces a release
# that is quietly missing one.
#
# # Determinism
#
# dietlibc and fat_io_lib are compiled here, so they are subject to the same
# rules as everything else: `SOURCE_DATE_EPOCH` is already exported by the
# caller, `ARFLAGS=Dcr` makes `ar` write deterministic archives, and the build
# runs at a fixed path inside the container.

set -euo pipefail

TOOLCHAINS="${TOOLCHAINS:-/toolchains}"
: "${SOURCE_DATE_EPOCH:?must be set by the caller}"

echo "== unpacking toolchains from $TOOLCHAINS"

unpack_once() {
    # target directory, archive name, tar flag
    local dir="$1" archive="$2" flag="$3"
    if [ -d "$dir" ]; then
        echo "  present: $dir"
        return 0
    fi
    if [ ! -f "$TOOLCHAINS/$archive" ]; then
        echo "  MISSING archive: $archive"
        return 1
    fi
    tar "$flag" "$TOOLCHAINS/$archive" -C /opt
    echo "  unpacked: $dir"
}

unpack_once /opt/gcc-linaro-7.4.1-2019.02-x86_64_aarch64-linux-gnu \
            gcc-linaro-7.4.1-2019.02-x86_64_aarch64-linux-gnu.tar.xz -xf || true
unpack_once /opt/aarch64--uclibc--stable-2020.08-1 \
            aarch64--uclibc--stable-2020.08-1.tar.bz2 -xf || true
unpack_once /opt/mips-loongson-gcc7.3-linux-gnu \
            mips-loongson-gcc7.3-2019.06-29-linux-gnu.tar.gz -xf || true

export PATH="$PATH:/opt/gcc-linaro-7.4.1-2019.02-x86_64_aarch64-linux-gnu/bin"
export PATH="$PATH:/opt/aarch64--uclibc--stable-2020.08-1/bin"
export PATH="$PATH:/opt/mips-loongson-gcc7.3-linux-gnu/2019.06-29/bin"

# ---------------------------------------------------------------------------
# dietlibc
# ---------------------------------------------------------------------------
#
# Built from the pinned source rather than downloaded as a binary, which makes
# it one of the few parts of this chain that is genuinely source-derived.
#
# Upstream's `DOC/installdietlibc.sh` does the same two builds. It is not called
# directly because it expects the tarball in the working directory and removes
# its own scratch directories with `rm -rf`, which is a poor thing to run
# against a path this script does not control.

build_dietlibc() {
    local archive="$TOOLCHAINS/dietlibc-0.34.tar.xz"
    if [ -d /opt/diet64 ] && [ -d /opt/diet32 ]; then
        echo "  present: /opt/diet32 and /opt/diet64"
        return 0
    fi
    if [ ! -f "$archive" ]; then
        echo "  MISSING archive: dietlibc-0.34.tar.xz"
        return 1
    fi

    local scratch
    scratch="$(mktemp -d)"

    echo "== building dietlibc (64-bit)"
    tar -xf "$archive" -C "$scratch"
    ( cd "$scratch/dietlibc-0.34" \
      && prefix=/opt/diet64 make -j"$(nproc)" >/dev/null 2>&1 \
      && prefix=/opt/diet64 make install >/dev/null 2>&1 ) || true
    rm -rf "$scratch/dietlibc-0.34"

    echo "== building dietlibc (32-bit)"
    tar -xf "$archive" -C "$scratch"
    ( cd "$scratch/dietlibc-0.34" \
      && sed -i 's/MYARCH:=.*/MYARCH=i386/' Makefile \
      && sed -i 's/CC=gcc/CC=gcc -m32/' Makefile \
      && prefix=/opt/diet32 make -j"$(nproc)" >/dev/null 2>&1 \
      && prefix=/opt/diet32 make install >/dev/null 2>&1 ) || true

    rm -rf "$scratch"

    [ -x /opt/diet64/bin/diet ] && echo "  built: /opt/diet64/bin/diet"
    [ -x /opt/diet32/bin/diet ] && echo "  built: /opt/diet32/bin/diet"
}

build_dietlibc || echo "  dietlibc unavailable; components needing it will be recorded as not built"

# ---------------------------------------------------------------------------
# fat_io_lib
# ---------------------------------------------------------------------------
#
# `vtoycli/build.sh` and `vtoyfat/build.sh` link against
# `fat_io_lib/lib/libfat_io_*.a` and do not build them. Those archives are not
# committed, so the scripts fail on a clean checkout with
# "No such file or directory", which is what happened on the first run here.
#
# `buildlib.sh` beside each of them is what produces them.

build_fat_io_lib() {
    local dir="$1"
    if [ ! -f "$dir/buildlib.sh" ]; then
        echo "  no buildlib.sh in $dir"
        return 1
    fi
    echo "== building fat_io_lib in $dir"
    ( cd "$dir" && bash buildlib.sh >/dev/null 2>&1 ) || true
    ls "$dir/lib/" 2>/dev/null | sed 's/^/  /' || echo "  produced nothing"
}

build_fat_io_lib vtoycli/fat_io_lib || true
build_fat_io_lib vtoyfat/fat_io_lib || true

echo ""
echo "== toolchains available"
for tool in gcc aarch64-linux-gnu-gcc aarch64-buildroot-linux-uclibc-gcc \
            mips64el-linux-musl-gcc mips-linux-gnu-gcc; do
    if command -v "$tool" >/dev/null 2>&1; then
        printf '  yes  %s\n' "$tool"
    else
        printf '  NO   %s\n' "$tool"
    fi
done
for diet in /opt/diet32/bin/diet /opt/diet64/bin/diet; do
    if [ -x "$diet" ]; then printf '  yes  %s\n' "$diet"; else printf '  NO   %s\n' "$diet"; fi
done
echo ""
