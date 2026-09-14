<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

<!-- The banner is SVG rather than a committed PNG, for the reason the whole
     project exists: a picture nobody can read is a binary blob with a nicer
     file extension. `assets/banner.svg` is text, every mark in it is a line you
     can read, and editing it produces a diff rather than an opaque new file. -->
![Ventoy-Reproducible: every binary compiled in public](assets/banner.svg)

# Ventoy-Reproducible

**Trust is not verification, and Ventoy asks for trust 1079 times.**

### → [tilas01.github.io/Ventoy-Reproducible](https://tilas01.github.io/Ventoy-Reproducible/)

[Ventoy](https://github.com/ventoy/Ventoy) is genuinely good software. It turns
a USB stick into something you drop ISO files onto and boot, it supports
hundreds of distributions, and it is free software under the GPL. This project
is a fork of it, it is not a criticism of it, and nothing here suggests its
authors have done anything wrong.

It exists because of one property that has nothing to do with intent.

## The problem

Ventoy's repository contains **1079 executable binaries**: 754 committed as
loose ELF and PE files, and a further 325 committed compressed, which a running
system unpacks and executes. Bootloaders, EFI drivers, 59 kernel modules,
Windows executables. When you install Ventoy, those exact files are written to
your USB stick and your computer runs them before your operating system
starts.

Not one of them was compiled by anybody outside the project.

There is no build you can watch, no log you can read, and no way to check that
`ventoy_x64.efi` in the repository is what its source code compiles to. You can
read every line of the C, and it tells you nothing about the binary sitting next
to it. The source being open does not make the binary open.

That is not an allegation, it is an absence. The Ventoy binaries may be
perfectly clean, and probably are. But "probably is" is not a security property,
and today no reader can do better than "probably is" no matter how careful they
are. Code that runs before your operating system, from a project used to install
operating systems, is a bad place to have to guess.

## What this fork does

Every binary is compiled by **GitHub Actions**, in the open, from source you can
read, in a container pinned by digest, with every compiler pinned by SHA-256.
Each one is built **twice**, in two differently named directories, and if the
two builds disagree by a single byte that file is reported as failing.

The result is a **signed manifest** naming every file, its size, its SHA-256 and
its BLAKE3, and a verdict for each. `ventoy-verify` checks the signature, then
checks every file. One command.

```bash
ventoy-verify release --key signing-key.asc
```

And the part most projects leave out: the manifest and the report **name every
file that did not reproduce and every file this project could not build at
all**. A gap nobody is told about is a gap nobody closes.

## Honest status

This project does not yet rebuild all 1079 files, and it will never claim to.

| | |
|---|---|
| Compiled from source here | the self-contained C tools, growing |
| Wired but not yet enabled | GRUB2 modules, the EDK2 EFI binaries |
| Cannot be built here, only pinned | 20 third-party binaries: the Rocky Linux shim, the imdisk driver, a TinyCore kernel, 7-Zip, memdisk |
| Needs a builder this project does not have | the BSD kernel modules, the Windows executables |
| No build instructions exist anywhere | 30 Linux kernel modules under `LiveCD/VTOY/ventoy/drivers/` |

Every release publishes a `reproducibility-report.md` with the current numbers
and the name of every file in each row. Read it before relying on any of this.

## What we found on the way

Deriving the inventory by machine rather than reading upstream's own
`BLOB_List.md` gives different numbers, and they are worth knowing:

```
executables in tree          1079
loose                         754
compressed                    325
paths named in blob list      182
named and present             176
present but undocumented      903
grub2 modules                 859
linux kernel modules           59
```

859 of the undocumented files are GRUB2 modules, which the list covers
collectively with a single "build grub2" instruction rather than naming
individually. That is reasonable, and it means a reader counting entries in the
table undercounts the tree roughly sixfold.

The 59 Linux kernel modules are the sharper case. They are committed `.ko` and
`.ko.xz` files that load into a running kernel, and the blob list does not
mention them at all.

Run it yourself:

```bash
python3 tools/inventory/inventory.py --print-counts
```

Checking upstream's compilers also turned up something in its favour, which
belongs here for the same reason the gaps do. Upstream re-hosts seven build
dependencies and downloads them with no verification at all. Two of them turn
out to be byte-identical to their canonical originals: `musl-1.2.1.tar.gz`
matches musl.libc.org exactly, and `grub-2.04.tar.xz` matches ftp.gnu.org
exactly. The edk2 archive comes straight from tianocore's own repository. The
other four, including three prebuilt GCC cross toolchains, have no canonical
source to compare against. All seven are now pinned in
[`tools/build/toolchains.lock`](tools/build/toolchains.lock).

## Install `ventoy-verify`

<details>
<summary><b>Linux, macOS, and the BSDs</b></summary>

```bash
# From a release
curl -LO https://github.com/tilas01/Ventoy-Reproducible/releases/latest/download/ventoy-verify-x86_64-unknown-linux-gnu
chmod +x ventoy-verify-x86_64-unknown-linux-gnu
sudo mv ventoy-verify-x86_64-unknown-linux-gnu /usr/local/bin/ventoy-verify

# Or build it, which is the point of the exercise
git clone https://github.com/tilas01/Ventoy-Reproducible
cd Ventoy-Reproducible/tools/ventoy-verify
cargo build --release --locked
./target/release/ventoy-verify --help
```

</details>

<details>
<summary><b>Windows</b></summary>

```powershell
# Download ventoy-verify-x86_64-pc-windows-msvc.exe from the releases page,
# then from the folder you put it in:
.\ventoy-verify.exe --help
```

Or build it with `cargo build --release --locked` in `tools\ventoy-verify`.

</details>

<details>
<summary><b>A window, if you would rather not use a terminal</b></summary>

```bash
cargo build --release -p ventoy-verify-gui
./target/release/ventoy-verify-gui
```

Choose the release folder, the manifest, and the public key. It shows the same
answer the command line does, and it never counts a file it did not build as a
file it verified.

</details>

## Verify a release

```bash
ventoy-verify release --root . --key signing-key.asc
```

That checks the signature over `manifest.json` first, and reads nothing from the
manifest until the signature passes. Then it hashes every file the manifest
names and reports what it found, failures first.

**There is no keyring, and that is the point.** `gpg --verify` exits zero for a
signature made by *any* key you have ever imported, so somebody who once
imported a key from a forum post gets a green tick from a file that person
signed. `ventoy-verify` takes one key file and verifies against exactly that
certificate. A signature by anything else is a failure, with both fingerprints
printed.

Exit codes are `0` for a pass, `1` for a verification failure, and `2` for
"the check could not be run", because in a script "the release is bad" and "you
typed the wrong path" deserve different reactions.

Without the tool, and checking rather less:

```bash
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum -c SHA256SUMS
```

## Platforms

Every platform Ventoy itself supports is in scope: **x86_64**, **i386**,
**aarch64** and **mips64el** for Linux, **Windows** 10 and 11 including ARM64,
and the BSD kernel modules for **FreeBSD**, **MidnightBSD**, **pfSense**,
**ClonOS** and **DragonFly**. `ventoy-verify` itself is built for seven targets
across Linux, Windows and macOS.

Reproducibility is per-target. A Linux x86-64 build reproducing says nothing
about aarch64, and the manifest never implies otherwise.

## Staying level with upstream

A scheduled workflow checks `ventoy/Ventoy` **every three hours**. When upstream
commits, this fork merges and rebuilds. When upstream publishes a release, this
fork builds and publishes a matching one tagged `<upstream tag>+repro.N`, so you
can always tell which Ventoy you are holding.

If a merge conflicts it stops and opens an issue rather than guessing. This is a
repository full of bootloaders.

## Documentation

| | |
|---|---|
| [`docs/WHITEPAPER.md`](docs/WHITEPAPER.md) | The full argument, the threat model, and what this does not prove |
| [`docs/REPRODUCIBLE_BUILDS.md`](docs/REPRODUCIBLE_BUILDS.md) | Every source of nondeterminism and the pin for each |
| [`docs/VERIFYING.md`](docs/VERIFYING.md) | Checking a release, in detail, including what each failure means |
| [`docs/SIGNING.md`](docs/SIGNING.md) | The signing key, its fingerprint, and how to check it against a second source |
| [`docs/UPSTREAM_SYNC.md`](docs/UPSTREAM_SYNC.md) | How this fork tracks Ventoy, and what happens when it cannot |

## What this does not prove

Said here rather than in an appendix, because it is the part that makes the rest
credible.

- A reproducible build proves a binary follows from its source under a given
  toolchain. It does **not** prove the source is safe. Nobody here has audited
  Ventoy's C.
- It does **not** prove the compiler is honest. Guarding against that needs
  diverse double-compilation, which this project does not do. Four of the seven
  pinned toolchains are prebuilt binaries with no canonical source.
- Pinning a compiler by hash fixes *which* unverified compiler you get. That is
  a smaller claim than it sounds, and it is the one being made.
- Twenty files are third-party signed binaries this project can only pin. They
  are counted separately from files it compiled, everywhere, always.
- Where our build differs from the binary upstream committed, that is reported
  as a difference to investigate. It is not evidence of wrongdoing, and the
  usual cause is a toolchain difference.

## Credits

Ventoy is written and maintained by **longpanda** and its contributors. All of
the functionality lives in their work; this fork adds a build pipeline, a
verification tool and documentation around it, and takes no credit for Ventoy
itself.

This fork is maintained by **tilas01**, who holds the copyright in the material
added here and is the sole author for licensing purposes.

Much of the code and documentation added by this fork was drafted with the help
of **Claude**, Anthropic's assistant, working to tilas01's direction. Nothing
reaches a release unread: every change is reviewed, built and tested before it
is committed. The credit is stated here, in the open, rather than scattered
through the commit log.

## Licence

**GPL-3.0-or-later.** See [`COPYING`](COPYING).

Ventoy's own source files carry the wording "either version 3 of the License,
or (at your option) any later version", so distributing this fork under
GPL-3.0-or-later is permitted by the licence upstream chose. The material this
fork adds is under the same terms.

The third-party binaries this repository pins but does not build are each under
their own licence and belong to their own projects. They are listed in
`manifest.json` with `"origin": "upstream-binary"`, which is also how the
verifier reports them.
