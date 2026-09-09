#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Generate the release signing key.
#
# Everything it writes goes to `gpg_secrets/`, which `.gitignore` excludes. The
# private key and the passphrase are meant to leave that folder exactly twice:
# into the maintainer's password manager, and into GitHub's encrypted secret
# store. The public key is meant to be published.
#
# Run once. Running it again makes a second key, which is a different identity
# and invalidates nothing about the first: rotating a signing key is a
# deliberate act with an announcement attached, not something a script does by
# being run twice.
#
# ============================================================================
# On "the strongest possible" settings
# ============================================================================
#
# Some of what people ask for here does not exist, and picking the real maximum
# is more useful than pretending otherwise.
#
# **RSA 4096 is the ceiling in practice.** GnuPG will not generate above 4096
# bits without being rebuilt, and larger RSA is not meaningfully stronger: the
# gap between 3072-bit RSA and anything an adversary could actually attack is
# already enormous, and past 4096 the cost lands entirely on the people
# verifying. There is no 8192-bit tier that buys security rather than latency.
#
# **AES-GCM is not an OpenPGP thing.** OpenPGP protects a private key with a
# symmetric cipher in CFB mode driven by a string-to-key function, and newer
# GnuPG offers OCB rather than GCM for message encryption. The real controls are
# which cipher, which digest, and how expensive the S2K is, and all three are
# set to their maxima below: AES-256, SHA-512, and an S2K count of 65011712,
# which is the largest value the format can encode.
#
# **The passphrase matters more than the key size.** A 4096-bit key behind a
# guessable passphrase is a guessable key. This script generates 256 bits of
# entropy from the system CSPRNG rather than letting anybody choose.
#
# ============================================================================
# Shape of the key
# ============================================================================
#
# A certify-only primary key with a separate signing subkey. The primary key's
# only job is to certify the subkey, so it can be kept offline; day to day, and
# in CI, only the signing subkey is needed. If CI is ever compromised, the
# subkey is revoked and replaced without the identity everybody trusts changing.

set -euo pipefail

OUT="${1:-gpg_secrets}"
NAME="${SIGNING_NAME:-Ventoy-Reproducible Release Signing}"
EMAIL="${SIGNING_EMAIL:-releases@ventoy-reproducible.invalid}"
COMMENT="${SIGNING_COMMENT:-Signs manifest.json and SHA256SUMS for every release}"

if [ -e "$OUT" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
    echo "error: $OUT already exists and is not empty." >&2
    echo "Generating a second key is a deliberate act. Move the old one aside" >&2
    echo "first, and read docs/SIGNING.md on rotation before you do." >&2
    exit 1
fi

mkdir -p "$OUT"
chmod 700 "$OUT"

# A private GnuPG home, so this never touches the operator's real keyring and
# never leaves anything behind in it.
export GNUPGHOME="$OUT/gnupghome"
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"

echo "==> generating a passphrase"

# 256 bits from the system CSPRNG, rendered as 43 base64 characters. Not a
# wordlist: a passphrase that lives in a password manager is never typed, so
# memorability buys nothing and costs entropy.
PASSPHRASE="$("${PYTHON:-python}" -c '
import base64
import secrets
print(base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip("="))
')"

umask 077
printf '%s\n' "$PASSPHRASE" > "$OUT/passphrase.txt"

echo "==> generating the primary key and signing subkey (this takes a minute)"

cat > "$GNUPGHOME/params" <<PARAMS
%echo Generating the Ventoy-Reproducible release signing key
Key-Type: RSA
Key-Length: 4096
Key-Usage: cert
Subkey-Type: RSA
Subkey-Length: 4096
Subkey-Usage: sign
Name-Real: $NAME
Name-Comment: $COMMENT
Name-Email: $EMAIL
Expire-Date: 3y
Passphrase: $PASSPHRASE
%commit
%echo done
PARAMS

# The S2K settings are what actually protect the private key at rest. AES-256,
# SHA-512, and 65011712 iterations, which is the maximum the OpenPGP format can
# encode in its single-byte count field.
gpg --batch \
    --pinentry-mode loopback \
    --s2k-cipher-algo AES256 \
    --s2k-digest-algo SHA512 \
    --s2k-mode 3 \
    --s2k-count 65011712 \
    --cert-digest-algo SHA512 \
    --digest-algo SHA512 \
    --gen-key "$GNUPGHOME/params"

rm -f "$GNUPGHOME/params"

FPR="$(gpg --list-secret-keys --with-colons | awk -F: '/^fpr:/ {print $10; exit}')"
echo "==> fingerprint: $FPR"

# Prefer SHA-512 and AES-256 in this key's self-signature, so that anything
# signing with it defaults to those rather than to SHA-1 era settings.
gpg --batch --pinentry-mode loopback --passphrase "$PASSPHRASE" \
    --command-fd 0 --edit-key "$FPR" <<EDIT >/dev/null 2>&1 || true
setpref SHA512 SHA384 SHA256 AES256 AES192 AES ZLIB BZIP2 ZIP Uncompressed
y
save
EDIT

echo "==> exporting"

# The public key, which is the only file here meant to be published.
gpg --armor --export "$FPR" > "$OUT/public-key.asc"

# The full secret key, for the maintainer's offline backup.
gpg --batch --pinentry-mode loopback --passphrase "$PASSPHRASE" \
    --armor --export-secret-keys "$FPR" > "$OUT/private-key-FULL.asc"

# The signing subkey only. This is what goes into CI: it can sign and it cannot
# certify, so a compromise of the runner cannot mint a new identity under this
# key.
gpg --batch --pinentry-mode loopback --passphrase "$PASSPHRASE" \
    --armor --export-secret-subkeys "$FPR" > "$OUT/private-subkey-FOR-CI.asc"

# The revocation certificate. GnuPG 2.1 and later writes one automatically at
# key creation, into `openpgp-revocs.d/`, which is better than asking for one
# through `--gen-revoke`: it exists before anything can go wrong, and the
# interactive prompt sequence that command wants is fragile to drive in batch.
#
# It is copied out so it sits beside the rest of the material rather than inside
# a keyring directory somebody might delete. It matters because a revocation
# certificate cannot be produced after the key is lost, which is exactly the
# situation it exists for.
if [ -f "$GNUPGHOME/openpgp-revocs.d/$FPR.rev" ]; then
    cp "$GNUPGHOME/openpgp-revocs.d/$FPR.rev" "$OUT/revocation-certificate.asc"
else
    echo "warning: no revocation certificate was generated; make one by hand" >&2
fi

printf '%s\n' "$FPR" > "$OUT/fingerprint.txt"

chmod 600 "$OUT"/*.asc "$OUT"/*.txt 2>/dev/null || true

cat <<SUMMARY

================================================================
  Key generated
================================================================

  Fingerprint   $FPR

  Files, all in $OUT/ which git ignores:

    public-key.asc              publish this, it is meant to be public
    private-subkey-FOR-CI.asc   goes into the GPG_PRIVATE_KEY secret
    private-key-FULL.asc        offline backup, never upload this anywhere
    passphrase.txt              goes into the GPG_PASSPHRASE secret
    revocation-certificate.asc  offline backup, for the day it is needed
    fingerprint.txt             the fingerprint, for pasting into docs

  Next: docs/SIGNING.md, section "Putting the key into GitHub".

  Then delete passphrase.txt and private-key-FULL.asc from this machine,
  once both are in your password manager. A secret in two places is twice
  as likely to leak and no more available.

================================================================
SUMMARY
