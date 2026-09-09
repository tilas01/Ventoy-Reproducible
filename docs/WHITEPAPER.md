<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Ventoy-Reproducible

**A fork of Ventoy whose binaries are compiled in public, and an honest account
of how far that gets you.**

Version 1, 2026-09-10. Against Ventoy 1.1.07.

---

## 1. The situation

Ventoy is free software. Its source is on GitHub, it has an active maintainer,
thousands of contributors' worth of issue reports, and a licence that guarantees
you the right to read every line. By every ordinary measure it is open.

Now try to answer a specific question: **is `INSTALL/ventoy/ventoy_x64.efi` the
file that this repository's C source compiles to?**

You cannot. Not because the answer is no, but because nothing in the repository,
the release process or the documentation lets you find out. That file is
committed to git as a finished binary. It was compiled once, on a computer you
have never seen, by a person you have never met, and every clone of the
repository since has copied those exact bytes forward.

This is not unusual. It is how a large amount of infrastructure software has
always worked. It is also, specifically here, a bootloader: code your computer
executes before your operating system exists, with full control of the machine,
installed by a tool whose entire purpose is putting operating systems on
computers.

### 1.1 How many files

Upstream documents its committed binaries in `BLOB_List.md`, a hand-written
table with 182 entries. Deriving the same set by machine, by reading the first
bytes of every tracked file, gives a different picture:

| | |
|---|---|
| Files in the tree with ELF or PE magic | **806** |
| Paths named in `BLOB_List.md` | 182 |
| Named and actually present | 176 |
| Present but not named anywhere | **630** |
| ...of those, GRUB2 modules under `INSTALL/grub/` | 574 |
| ...of those, everything else | 56 |
| ...of those, kernel modules with no recorded build instructions | 30 |

Reproduce this with `python3 tools/inventory/inventory.py --print-counts`.

The 574 GRUB2 modules are covered collectively by a single "build grub2"
instruction rather than named individually, which is reasonable. The effect is
still that a reader counting the table undercounts the tree by a factor of four.

The 30 kernel modules under `LiveCD/VTOY/ventoy/drivers/` are `.ko` files with
no build instruction recorded anywhere in the repository, and no entry in the
blob list. They are the clearest single example of the problem: committed
binaries that load into a running kernel, whose provenance is not documented at
all.

### 1.2 This is not an accusation

Every binary in Ventoy may be exactly what its source compiles to. There is no
evidence otherwise, this project has looked for none, and it would be dishonest
to imply any exists. The Ventoy authors have built something genuinely useful
and given it away.

The argument here does not depend on suspicion, and it would be weaker if it
did. It is this: **an unverifiable binary is unverifiable regardless of who
produced it.** The property that matters is not the maintainer's honesty, which
cannot be checked, but whether a reader has any way to check the artefact, which
currently they do not.

A drifted inventory list is the ordinary consequence of maintaining a table by
hand beside a set a script generates. It is reported here because a project that
publishes only the findings that flatter its own thesis is not doing
verification, it is doing advocacy.

---

## 2. Threat model

### 2.1 What this project defends against

**A compromised maintainer machine.** The single most common way real supply
chain attacks happen. A developer's laptop is compromised, the attacker modifies
a binary before it is committed, and the source stays clean so a code review
finds nothing. Building in public CI from public source means the binary is
derived from what everybody can read, and the machine that derives it is
destroyed afterwards.

**A compromised account, publishing a modified release.** Publishing a binary
requires it to match a build that GitHub Actions performed and logged, and the
manifest is signed by a key held outside GitHub.

**Silent divergence between source and binary over time.** A binary committed
once and copied forward for years is never re-derived. Rebuilding on every
upstream commit means a divergence appears as a difference in a report rather
than as nothing at all.

**Substitution of a build dependency.** Upstream's CI downloads seven archives,
five of which are compilers, with no verification. This fork pins all seven by
SHA-256 and refuses to build if a byte differs.

### 2.2 What this project does not defend against

Stated with the same emphasis, because a threat model that lists only successes
is marketing.

**A malicious compiler.** Four of the seven pinned toolchains are prebuilt GCC
binaries with no canonical source to compare against. A compiler that inserts a
backdoor produces a perfectly reproducible backdoored binary: two builds agree,
the manifest says `reproduced`, and the claim is satisfied while the artefact is
hostile. Ken Thompson described this in 1984 and it has not stopped being true.
Defending against it needs diverse double-compilation, building the same source
with independently produced compilers and comparing. This project does not do
that. It is the single largest gap.

**A bug or backdoor in the source.** Reproducibility says the binary follows
from the source. It says nothing about whether the source is safe. Nobody here
has audited Ventoy's C, and this project makes no claim about it.

**A compromised GitHub.** The builds run on GitHub's infrastructure, using
GitHub's runner images, and the results are published on GitHub. An attacker
inside that infrastructure could produce a manifest that says whatever they
like. The detached OpenPGP signature narrows this: the private key is held
outside GitHub, so a forged build is unsigned. It does not eliminate it.

**Twenty third-party binaries.** The Rocky Linux shim, the imdisk driver, a
TinyCore kernel, 7-Zip, memdisk and others cannot be built here at all. They are
signed binaries from other projects. This project pins and records their hashes,
which fixes which file you get and derives nothing. They are counted separately
from files this project compiled, in the manifest, in the reports, in the
verifier's output and in its summary lines. There is deliberately no field that
merges the two counts.

**Everything not yet wired.** Today this fork compiles the self-contained C
tools. The GRUB2 and EDK2 builds are written and not yet enabled; the BSD
kernel modules need a FreeBSD builder; the Windows executables need a Windows
job. Each unbuilt path is listed by name in every manifest with a reason.

---

## 3. Design

### 3.1 The claim, stated precisely

> For a given commit, a given blob path and a given toolchain lock, two
> independent builds produce byte-identical output, and the SHA-256 of that
> output is published, signed, before anybody downloads it.

Narrower than "this binary is safe", and it is the whole claim.

### 3.2 Sources of nondeterminism, and the pin for each

Most of what makes a C build unreproducible is ambient rather than written down.

| Source | Pin |
|---|---|
| Compiler version | A container image pinned by digest, never a floating tag |
| Cross toolchains | `toolchains.lock`, SHA-256 each, fetched not vendored |
| Timestamps in archives | `SOURCE_DATE_EPOCH`, taken from the commit date |
| Timestamps in `ar` archives | `ARFLAGS=Dcr`, deterministic mode |
| Build ids | `-Wl,--build-id=none` |
| Absolute paths in binaries | `-ffile-prefix-map`, set by the environment |
| Compiler version strings | `-fno-ident` |
| Filesystem ordering | `LC_ALL=C`, and sorted input to every archive step |
| Locale and timezone | `LC_ALL=C`, `TZ=UTC`, exported before anything runs |
| Section ordering | `-Wl,--sort-section=name` |
| File permission bits | `umask 022` |
| Tar metadata | `--owner=0 --group=0 --numeric-owner --sort=name` |

Path remapping lives in the environment rather than a checked-in config file.
Hardcoding one contributor's home directory would make the build reproducible
only for them, which is the opposite of the point.

The base image is CentOS 7, matching upstream, and this is not nostalgia. The
glibc version determines the symbol versions in every binary produced, so a
newer base silently produces binaries that will not run on the systems Ventoy
supports.

### 3.3 The double build

Every component is built twice, in `/build/alpha` and
`/build/beta-with-a-longer-name`. The names differ in length on purpose:
anything that embeds its own build path shows up as a difference, which building
twice in the same directory would never catch.

A file that differs between the two builds is recorded as `differs` and named in
the report. **It does not fail the run.** Failing would hide the evidence rather
than publish it, and the only real asset this project has is that its bad news
is legible.

### 3.4 Three questions, kept apart

There are three things somebody might mean by "did it work", and merging them is
how a reproducibility report becomes a press release:

1. **Did our two builds agree with each other?** Recorded as `verdict`.
2. **Did our build match the binary upstream committed?** Recorded as
   `matches_upstream`.
3. **Did we build it at all?** Recorded as `origin`.

A file can reproduce perfectly and still differ from upstream's committed copy.
That is the interesting case, and a single pass/fail would bury it. A difference
from upstream is reported as a fact to investigate, not as an accusation; the
usual cause is a toolchain difference, and closing it is ordinary work.

### 3.5 What gets signed

One file: `manifest.json`. It names every artefact with its size and both
digests, so a signature over it is a signature over the whole collection.

Signing 806 files individually would produce 806 signatures nobody checks, and
would leave the *set* unsigned: a file could be removed from a release and no
signature would fail. With one manifest, a missing file is a verification
failure.

`SHA256SUMS` is generated from the manifest rather than computed separately, so
the two cannot drift, and it is signed as well for people whose habits are built
around `sha256sum -c`.

### 3.6 The verifier

`ventoy-verify` is four Rust crates, and the split is a security boundary rather
than a filing decision. `core` hashes and compares and has no signature code, no
network code and no interface. `sig` checks detached signatures and never
touches the filesystem. `cli` and `gui` are the only crates allowed to print or
exit, and neither of the first two depends on them. The window therefore cannot
reach a file except through `core`: one place where a path becomes a read, one
place to audit.

Every crate root carries `#![forbid(unsafe_code)]`, not `deny`, so no module can
opt back in. CI checks for both the attribute and the keyword.

**It has no keyring.** `gpg --verify` exits zero when a signature was made by any
key the user has ever imported, and prints its trust warning on a line most
people scroll past. Somebody who once imported a key from a forum post gets a
green tick. `ventoy-verify` takes exactly one public key file and verifies
against exactly that certificate; a signature by anything else is a failure
naming both fingerprints. The trust decision is made once, visibly, when the
reader chooses the key file.

The signature is checked **before** the manifest is parsed, and the manifest's
schema and digests are validated **before** any file it names is opened. In the
other order, a hostile manifest has already directed 806 file reads before
anything questioned where it came from. Manifest paths are refused if they are
absolute or contain a parent component, because a manifest downloaded from a
release page is untrusted input.

### 3.7 Staying level with upstream

A scheduled workflow checks `ventoy/Ventoy` every three hours, merges new
commits, and rebuilds. It asks upstream what its default branch is called rather
than assuming, because projects rename and a hardcoded name fails silently by
syncing nothing for months.

It merges rather than rebases: rebasing would rewrite this fork's published
history on every upstream commit, which breaks every clone and every signature
over a commit.

A conflict stops the run and opens one issue rather than guessing at a
resolution. Releases are tagged `<upstream tag>+repro.N`, so the upstream
version a user is actually looking for stays visible.

---

## 4. Results

As of 2026-09-10, against Ventoy 1.1.07:

- **806** executables identified in the tree by machine.
- **7** build dependencies pinned by SHA-256 that upstream fetched unverified.
- **2** of those 7 confirmed byte-identical to their canonical upstream
  releases: musl 1.2.1 against musl.libc.org, GRUB 2.04 against ftp.gnu.org.
  This is evidence in upstream's favour and is published for the same reason
  the gaps are.
- **1** of those 7 canonical by URL: the edk2 archive, from tianocore's own
  repository.
- **4** of those 7 with no canonical source available for comparison, including
  three prebuilt GCC cross toolchains.
- **20** files identified as third-party binaries that cannot be built here at
  all, only pinned.
- **30** kernel modules found with no build instructions recorded anywhere.

Per-release figures for what actually reproduced are published in
`reproducibility-report.md` with every release. They are not repeated here,
because a number in a document is stale the moment it is written and a number in
a release is not.

---

## 5. Limits, restated

If you read only one section, read this one.

1. A reproducible build proves a binary follows from its source **under a given
   toolchain**. It does not prove the source is safe.
2. It does not prove the compiler is honest. Four of seven pinned toolchains are
   prebuilt binaries. Diverse double-compilation is not done here.
3. Pinning a compiler by hash fixes *which* unverified compiler you get. That is
   a smaller claim than it sounds.
4. Reproducibility is per-target. An x86-64 build reproducing says nothing about
   aarch64.
5. Twenty files can only be pinned, never built here, and are counted
   separately everywhere.
6. Most of the 806 are not yet built by this project. Each unbuilt path is named
   with a reason in every manifest.
7. The builds run on GitHub's infrastructure. The detached signature narrows
   what a compromise there could achieve; it does not eliminate it.
8. Signing happens after the build and is not part of the reproducible artefact.
   Verify the hash first, then the signature over the hash file.

---

## 6. Licence

GPL-3.0-or-later. Ventoy's own source headers carry "either version 3 of the
License, or (at your option) any later version", so this is permitted by the
licence upstream chose.

Ventoy is the work of longpanda and its contributors. This fork adds a build
pipeline, a verification tool and documentation, and claims none of the
functionality that makes Ventoy worth using.
