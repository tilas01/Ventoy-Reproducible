#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Compile Ventoy's committed binaries from source, deterministically.
#
# Runs inside the pinned CentOS 7 container. Everything it produces goes to the
# directory given as the first argument, under `tree/`, laid out with the same
# paths the files have in the repository, so the result can be compared against
# the committed blobs path by path.
#
# # The environment is the reproducibility
#
# Most of what makes a C build unreproducible is ambient: the clock, the locale,
# the working directory, the order readdir happens to return, the hostname the
# linker stamps in a build id. Every one of those is nailed down below, before
# any compiler runs, because a flag added to one Makefile fixes one Makefile and
# an environment fixes all of them.
#
# # Honesty about coverage
#
# This does not build all 1079 executables in the tree, and the manifest never
# claims it does. Components are wired in one at a time, each with its own
# recipe below; anything not named here is recorded as `not-built` with a
# reason. Growing this list is the main work of the project and the reports say
# where it currently stands.

set -euo pipefail

OUT="${1:?usage: build-blobs.sh OUTPUT_DIR}"
TREE="$OUT/tree"
LOGS="$OUT/logs"
mkdir -p "$TREE" "$LOGS"

# ---------------------------------------------------------------------------
# The environment, fixed before anything is compiled
# ---------------------------------------------------------------------------

# Without a source date, every archive step stamps "now" and nothing reproduces.
: "${SOURCE_DATE_EPOCH:?SOURCE_DATE_EPOCH must be set from the commit date}"
export SOURCE_DATE_EPOCH

export TZ=UTC
export LC_ALL=C
export LANG=C
# `sort` orders differently under different locales, and a differently ordered
# archive is a different archive.
export LC_COLLATE=C

# The build directory is deliberately different between the two passes, so that
# anything embedding its own path shows up as a difference. Remapping it back to
# a fixed string is what makes the two agree when nothing is actually wrong.
BUILD_ROOT="$(pwd)"
export SOURCE_PREFIX_MAP="-ffile-prefix-map=${BUILD_ROOT}=/ventoy"

# Flags every component inherits.
#   --build-id=none    a build id is a hash of inputs including timestamps
#   -Wl,--sort-section a stable section order rather than link order
#   -fno-ident         drops the compiler version string from a comment section
export CFLAGS_REPRO="${SOURCE_PREFIX_MAP} -fno-ident -fdebug-prefix-map=${BUILD_ROOT}=/ventoy"
export LDFLAGS_REPRO="-Wl,--build-id=none -Wl,--sort-section=name"

# `ar` writes timestamps, uids and gids into every archive member unless told
# not to. `D` is deterministic mode. Setting it here covers every Makefile that
# calls ar without thinking about it.
export ARFLAGS="Dcr"
export TAR_OPTIONS="--owner=0 --group=0 --numeric-owner --mtime=@${SOURCE_DATE_EPOCH} --sort=name"

# umask affects the permission bits recorded in archives.
umask 022

echo "build root:        $BUILD_ROOT"
echo "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH ($(date -u -d "@$SOURCE_DATE_EPOCH" 2>/dev/null || true))"
echo "toolchains:        /toolchains"
echo ""

# Unpack the pinned cross toolchains, build dietlibc from its pinned source,
# and build the fat_io_lib archives that two components link against and none
# of them builds. Sourced rather than run, because it exports PATH.
#
# shellcheck source=tools/build/prepare-toolchains.sh
. "$(dirname "${BASH_SOURCE[0]}")/prepare-toolchains.sh"

# ---------------------------------------------------------------------------
# Recipes
# ---------------------------------------------------------------------------
#
# Each entry is: a name, the script that builds it, and the paths it is expected
# to produce. A recipe that runs and produces nothing is a failure, not a pass,
# because a silently empty build is how a component drops out of the set without
# anybody noticing.

record_skip() {
    # name, reason
    printf '%s\t%s\n' "$1" "$2" >> "$OUT/not-built.tsv"
}

record_built() {
    printf '%s\t%s\n' "$1" "$2" >> "$OUT/built.tsv"
}

: > "$OUT/not-built.tsv"
: > "$OUT/built.tsv"

# Copy a produced file into the output tree at its repository path.
collect() {
    local src="$1" rel="$2" recipe="$3"
    if [ ! -f "$src" ]; then
        echo "  MISSING after build: $rel"
        record_skip "$rel" "recipe '$recipe' ran but did not produce this file"
        return 1
    fi
    mkdir -p "$TREE/$(dirname "$rel")"
    cp -a "$src" "$TREE/$rel"
    record_built "$rel" "$recipe"
    echo "  built: $rel"
}

run_recipe() {
    local name="$1" script="$2"
    echo "== $name"
    if [ ! -f "$script" ]; then
        echo "  no such script: $script"
        record_skip "$name" "build script $script is absent from this tree"
        return 1
    fi
    if ! ( cd "$(dirname "$script")" && bash "$(basename "$script")" ) \
            > "$LOGS/$name.log" 2>&1; then
        echo "  FAILED, see logs/$name.log"
        tail -20 "$LOGS/$name.log" || true
        record_skip "$name" "build script failed; see logs/$name.log"
        return 1
    fi
    return 0
}

# ---------------------------------------------------------------------------
# What can be built with the toolchains this project has pinned
# ---------------------------------------------------------------------------
#
# The first real run of this script found the difference between "a build script
# exists" and "a build script can run". Four of the six components wired here
# failed, for two reasons worth separating.
#
# `vtoycli` and `vtoyfat` link against `fat_io_lib/lib/libfat_io_*.a`, which is
# not committed and which their own `build.sh` does not build. That is fixable
# and is fixed: `prepare-toolchains.sh` runs the `buildlib.sh` beside each.
#
# `vtoyfat` and `vtoygpt` also call `mips64el-linux-musl-gcc`. That compiler
# lives at `/opt/mips64el-linux-musl-gcc730`, which is on the PATH upstream's
# `all_in_one.sh` sets and is not among the seven archives upstream's CI
# downloads. It is not fixable from anything in this repository, so those two
# are recorded as not built, by name, with that reason.

if run_recipe vtoytool VtoyTool/build.sh; then
    collect VtoyTool/vtoytool/00/vtoytool_32 VtoyTool/vtoytool/00/vtoytool_32 vtoytool || true
    collect VtoyTool/vtoytool/00/vtoytool_64 VtoyTool/vtoytool/00/vtoytool_64 vtoytool || true
fi

if run_recipe vlnk Vlnk/build.sh; then
    for arch in aarch64 i386 mips64el x86_64; do
        collect "INSTALL/tool/$arch/vlnk" "INSTALL/tool/$arch/vlnk" vlnk || true
    done
fi

if run_recipe vtoycli vtoycli/build.sh; then
    for arch in aarch64 i386 mips64el x86_64; do
        collect "INSTALL/tool/$arch/vtoycli" "INSTALL/tool/$arch/vtoycli" vtoycli || true
    done
fi

if run_recipe vblade VBLADE/vblade-master/build.sh; then
    for suffix in 32 64 aa64; do
        collect "VBLADE/vblade-master/vblade_$suffix" \
                "VBLADE/vblade-master/vblade_$suffix" vblade || true
    done
fi

# ---------------------------------------------------------------------------
# What cannot be built, and why
# ---------------------------------------------------------------------------
#
# Naming each one is the point. "Some components are not yet built" is not a
# status anybody can act on; a path and a reason is.

MUSL_REASON="needs mips64el-linux-musl-gcc, which is on upstream's build PATH \
but is not among the seven archives upstream's CI downloads and cannot be \
built from anything in this repository"

record_skip "vtoyfat" "$MUSL_REASON"
record_skip "vtoygpt" "$MUSL_REASON"

record_skip "INSTALL/EFI/BOOT/BOOTX64.EFI"     "third-party signed shim from a Rocky Linux ISO; cannot be built here, only pinned"
record_skip "INSTALL/EFI/BOOT/mmx64.efi"       "third-party signed binary from a Rocky Linux ISO; cannot be built here, only pinned"
record_skip "INSTALL/EFI/BOOT/BOOTIA32.EFI"    "third-party binary from Super-UEFIinSecureBoot-Disk; cannot be built here, only pinned"
record_skip "INSTALL/EFI/BOOT/grubia32.efi"    "third-party binary from Super-UEFIinSecureBoot-Disk; cannot be built here, only pinned"
record_skip "INSTALL/EFI/BOOT/mmia32.efi"      "third-party binary from Super-UEFIinSecureBoot-Disk; cannot be built here, only pinned"
record_skip "INSTALL/ventoy/imdisk"            "third-party Windows driver, Authenticode signed; cannot be built here, only pinned"
record_skip "INSTALL/ventoy/memdisk"           "third-party binary from the syslinux project; cannot be built here, only pinned"
record_skip "INSTALL/ventoy/7z"                "third-party binary from the 7-Zip project; cannot be built here, only pinned"
record_skip "LiveCD/ISO/EFI/boot/vmlinuz64"    "third-party kernel from TinyCore; cannot be built here, only pinned"
record_skip "INSTALL/grub"                     "GRUB2 modules; the cross build is not yet enabled in CI"
record_skip "INSTALL/ventoy/ventoy_x64.efi"    "EDK2 build; not yet enabled in CI"
record_skip "Unix/ventoy_unix"                 "BSD kernel modules; need a FreeBSD builder, which a Linux container is not"
record_skip "INSTALL/Ventoy2Disk.exe"          "Windows build; needs a Windows job, not this container"
record_skip "LiveCD/VTOY/ventoy/drivers"       "59 Linux kernel modules with no build instructions recorded anywhere upstream"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

built=$(wc -l < "$OUT/built.tsv" | tr -d ' ')
skipped=$(wc -l < "$OUT/not-built.tsv" | tr -d ' ')

echo ""
echo "built:     $built files"
echo "not built: $skipped entries, each with a reason in not-built.tsv"

# Producing nothing at all means the container or the recipes are broken, and
# publishing an empty "build" would be worse than failing.
if [ "$built" -eq 0 ]; then
    echo "error: nothing was built at all; refusing to report success" >&2
    exit 1
fi
