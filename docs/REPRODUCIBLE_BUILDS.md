<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Reproducible builds

A bootloader you cannot rebuild is a bootloader you are trusting on faith.
Reproducible builds close the gap between "the source is open" and "the binary I
am about to boot is that source".

## The claim, stated precisely

> For a given commit, a given blob path and a given toolchain lock, two
> independent builds produce byte-identical output, and the SHA-256 of that
> output is published, signed, before anybody downloads it.

That is the whole claim. It is deliberately narrower than "this binary is safe",
and [`WHITEPAPER.md`](WHITEPAPER.md) says at length why.

## Rebuild a release yourself

```bash
git clone https://github.com/tilas01/Ventoy-Reproducible
cd Ventoy-Reproducible
git checkout 1.1.07+repro.1

export SOURCE_DATE_EPOCH=$(git log -1 --pretty=%ct)
bash tools/build/fetch-toolchains.sh toolchains/cache

docker run --rm \
  -v "$PWD:/ventoy:ro" \
  -v "$PWD/toolchains/cache:/toolchains:ro" \
  -v "$PWD/out:/out" \
  -e SOURCE_DATE_EPOCH -e LC_ALL=C -e TZ=UTC \
  -e VTOY_WORKDIR=/build/alpha \
  ventoy-reproducible-builder:centos7 \
  /bin/bash -c 'cp -a /ventoy "$VTOY_WORKDIR" && cd "$VTOY_WORKDIR" && bash tools/build/build-blobs.sh /out'
```

Then compare what you got with what was published:

```bash
ventoy-verify manifest --root out/tree --manifest manifest.json
```

If a file differs, that is worth an issue. Include your `docker --version`, the
image digest you actually pulled, and the two hashes.

## Sources of nondeterminism, and the pin for each

Most of what makes a C build unreproducible is ambient rather than written into
any Makefile.

| Source | Pin |
|---|---|
| Compiler version | A container image pinned by digest, never a floating tag |
| Cross toolchains | `toolchains.lock`, SHA-256 each, fetched not vendored |
| Timestamps in archives | `SOURCE_DATE_EPOCH`, from the commit date |
| Timestamps in `ar` archives | `ARFLAGS=Dcr`, deterministic mode |
| Build ids | `-Wl,--build-id=none` |
| Absolute paths in binaries | `-ffile-prefix-map`, set by the environment |
| Compiler version strings | `-fno-ident` |
| Filesystem ordering | `LC_ALL=C` and sorted input to every archive step |
| Locale and timezone | `LC_ALL=C`, `TZ=UTC`, exported before anything runs |
| Section ordering | `-Wl,--sort-section=name` |
| File permission bits | `umask 022` |
| Tar metadata | `--owner=0 --group=0 --numeric-owner --sort=name` |

Path remapping lives in the environment rather than in a checked-in config file,
because hardcoding one contributor's home directory would make the build
reproducible only for them.

### Why CentOS 7

Not nostalgia. The glibc version determines the symbol versions recorded in
every binary produced, so building on a modern base silently produces binaries
that will not run on the systems Ventoy supports. Matching upstream's base is
the only way to produce a comparable artefact.

The image is pinned by digest rather than by the `centos:7` tag, because a
mutable base makes the word reproducible meaningless.

## The double build

Every component is built twice, in `/build/alpha` and in
`/build/beta-with-a-longer-name`.

The directory names differ in length deliberately. Anything that embeds its own
build path then shows up as a difference, which building twice in the same
directory would never catch. A file that differs is recorded as `differs` and
named in the report.

**A difference does not fail the build.** It is recorded, published and named.
Failing would hide the evidence rather than publish it.

## Three questions, kept apart

| Question | Field |
|---|---|
| Did our two builds agree with each other? | `verdict` |
| Did our build match the binary upstream committed? | `matches_upstream` |
| Did we build it at all? | `origin` |

A file can reproduce perfectly and still differ from upstream's committed copy.
That is the interesting case and a single pass/fail would bury it.

A difference from upstream is **not** evidence of wrongdoing. The usual cause is
a different compiler version or build host, and closing that gap is ordinary
work. It is published because a gap nobody is told about is a gap nobody closes.

## The toolchains

Upstream's `INSTALL/docker_ci_build.sh` begins by downloading seven archives
with `wget -q` and no verification. Five are compilers.

All seven are now pinned in [`../tools/build/toolchains.lock`](../tools/build/toolchains.lock),
and `fetch-toolchains.sh` refuses to build when a byte differs. Checking them
against canonical sources found:

| Archive | Status |
|---|---|
| `musl-1.2.1.tar.gz` | Byte-identical to musl.libc.org |
| `grub-2.04.tar.xz` | Byte-identical to ftp.gnu.org |
| `edk2-stable201911` | Canonical by URL, from tianocore's own repository |
| `dietlibc-0.34.tar.xz` | No canonical source reachable for comparison |
| `gcc-linaro-7.4.1-...` | Prebuilt binary toolchain, no canonical source |
| `aarch64--uclibc--stable-...` | Prebuilt binary toolchain, no canonical source |
| `mips-loongson-gcc7.3-...` | Prebuilt binary toolchain, no canonical source |

The first three are good news and are published as such. The last four are the
largest remaining hole in this project's chain: pinning tells you **which**
unverified compiler you got, not that the compiler is honest.

## Known limits

- **Reproducibility is per-target.** An x86-64 build reproducing says nothing
  about aarch64.
- **A reproducible build does not mean a safe binary.** It means the binary
  follows from the source. Nobody here has audited Ventoy's C.
- **It does not mean an honest compiler.** See the toolchain table above.
  Diverse double-compilation would address this and is not done here.
- **Twenty files cannot be built here at all.** They are third-party signed
  binaries. Pinning records which file you got and derives nothing.
- **Most of the 1079 are not yet built by this project.** Every unbuilt path is
  named with a reason in every manifest, rather than omitted.
- **Signing is not part of the reproducible artefact.** It happens after the
  build. Verify the hash first, then the signature over the hash file.
- **The linker version matters.** A different system linker can produce a
  different binary from identical object files, which is why the image is
  pinned by digest and the digest is recorded in every manifest.

## The verifier reproduces too

`ventoy-verify` is built twice in CI, in different directories, and the job
fails if one byte differs. A tool whose job is to tell you whether a binary is
reproducible has no standing if its own binary is not.

Its release profile sets `codegen-units = 1` because parallel codegen reorders
symbols between runs, `lto = "fat"`, `panic = "abort"` so there are no unwinding
tables to differ, `strip = true` because symbol tables carry paths and ordering,
and `incremental = false` because incremental state is per-machine by
construction.
