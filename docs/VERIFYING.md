<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Verifying a release

A bootloader you cannot check is a bootloader you are trusting on faith. This
page is how to stop doing that, and what each way it can fail actually means.

## The short version

```bash
ventoy-verify release --root . --key signing-key.asc
```

Exit code `0` means every file matches the signed manifest. Anything else means
stop and read the output.

## What that command actually does, in order

The order is the security property, not an implementation detail.

1. **Loads one public key.** Exactly one, from the file you named.
2. **Checks the detached signature over `manifest.json`.** If this fails,
   nothing below runs. The manifest is not parsed, no file is opened.
3. **Parses the manifest and validates every digest in it** before opening
   anything. A malformed entry at position 500 is caught before file 1 is read.
4. **Refuses any path that escapes the root.** A manifest is downloaded from a
   release page, which makes it untrusted input. A path of `../../etc/passwd`
   is rejected outright.
5. **Hashes every file** with SHA-256 and BLAKE3, in parallel, and compares
   both. Both must agree; publishing two digests and checking one would make
   the second decorative.
6. **Prints failures first**, then counts, then a verdict.

Doing this in the other order, hashing first and checking the signature at the
end, means a hostile manifest has already directed 806 file reads before
anything questioned where it came from.

## Reading the output

```
OK signature over manifest.json is good
   made by a signing subkey of 2666 F714 9F1F 786C AFE6 B452 D205 837B F772 13F9

806 files checked
  806 matched the manifest
    786 compiled by this project's CI
    20 third-party binaries, pinned by hash and not built here

PASSED. every file matches the manifest. Some were not built here; see the
counts above.
```

The counts are printed separately and are never added together. There is no
line saying "806 verified", because 20 of those files are third-party binaries
nobody here compiled, and presenting them as equally verified would be false.

## Exit codes

| Code | Meaning | What to do |
|---|---|---|
| `0` | Everything matched | Nothing |
| `1` | The check ran and something failed | Read the failures; do not use the release |
| `2` | The check could not be run | You probably typed a wrong path |

The difference between `1` and `2` matters in a script. "The release is bad" and
"the key file does not exist" deserve different reactions, and a tool that
returns the same code for both makes that impossible.

## What each failure means

### `the signature was made by key X, which is not Y`

The manifest was signed by a key that is not the one you supplied. This is the
message that matters most. It is the difference between "this file is
authentic" and "this file is signed by somebody, and it was not the
maintainer".

If you fetched the key from the release page, fetch it from the second source
named in [`SIGNING.md`](SIGNING.md) instead and compare the fingerprints. A key
published alongside the thing it signs proves nothing on its own.

### `the signature does not match the contents of manifest.json`

The manifest was altered after it was signed, or the download is corrupt. Fetch
both again. If it happens twice from a clean network, do not use the release.

### `... is not a valid armoured OpenPGP signature`

Almost always a truncated download or an HTML error page saved with a `.asc`
name. Check the file size first.

### `FAIL <path>: contents differ`

A file on disk is not the file the manifest describes, and the manifest's
signature was good. Either your copy was modified after download, or the
release was assembled from something other than what was signed. The expected
and actual digests are both printed so you can tell a truncated file from a
substituted one: a truncation shows a different size, a substitution usually
does not.

### `FAIL <path>: not present`

The manifest names a file the release does not contain. This is why the whole
set is signed as one manifest rather than file by file: with individual
signatures, a removed file takes its signature with it and nothing fails.

### `(third-party binary, reproducibility not established)`

Not a failure. It appears beside a file this project pins but cannot build,
such as the Rocky Linux shim or the imdisk driver. The file matches what was
published; nobody here compiled it.

## Checking your own build instead

If you have built the tree yourself and want to compare against a manifest
without any signature involved:

```bash
ventoy-verify manifest --root . --manifest manifest.json
```

There is no pretence of authenticity in this mode, which is the point of having
it separate.

## The strict flag

```bash
ventoy-verify release --key signing-key.asc --strict
```

`--strict` additionally requires that every file was compiled by this project's
CI **and** reproduced across two builds. A genuine, correctly signed release
will fail this today, because 20 of its files are third-party binaries that
cannot be built here at all. That is the expected result, not a bug, and the
flag exists so the difference between "intact" and "fully derived from source"
can be asked about separately.

## Without `ventoy-verify`

```bash
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum -c SHA256SUMS
```

This checks less, in two ways worth knowing about.

`gpg --verify` succeeds for a signature made by **any** key in your keyring, and
prints its warning about trust on a line that is easy to miss. If you have ever
imported a key you should not have, this passes. Check the fingerprint it
reports against [`SIGNING.md`](SIGNING.md) by eye, every time.

`SHA256SUMS` carries one digest per file rather than two, and carries no
information about which files this project actually compiled. It cannot tell you
that 20 of them are third-party binaries.

## Checking one file against a digest

For when somebody has given you a hash and a file and nothing else:

```bash
ventoy-verify check ventoy_x64.efi --sha256 ba7816bf8f01...
ventoy-verify hash ventoy_x64.efi
```

`hash` prints in the same shape `sha256sum` does, so a line can be pasted
straight into a `SHA256SUMS` file or compared by eye.

## In a script

```bash
ventoy-verify release --key signing-key.asc --format json > result.json
```

JSON output is the same data the terminal shows, including the separate counts.
Combine with `--quiet` to print nothing on success.
