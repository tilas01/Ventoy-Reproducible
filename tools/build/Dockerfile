# SPDX-License-Identifier: GPL-3.0-or-later
#
# The builder. CentOS 7, pinned by digest, with a compiler.
#
# # Why CentOS 7 in 2026
#
# Not nostalgia. The glibc version decides the symbol versions recorded in every
# binary produced, so building Ventoy's components on a modern base silently
# produces binaries that refuse to start on the older systems Ventoy supports.
# Upstream builds on CentOS 7, and matching it is the only way to produce an
# artefact comparable with theirs.
#
# # Why a Dockerfile at all, rather than running the base image directly
#
# An earlier version of `blobs.yml` ran `docker run` against the bare CentOS
# image and called `build-blobs.sh` inside it. That could never have worked:
# the base image has no gcc, no make and no binutils. The failure would have
# been a pile of "command not found" rather than anything informative, and it
# was masked by a second bug, an invented image digest that meant the container
# never started at all.
#
# # CentOS 7 is end of life, and that is why the URLs are rewritten
#
# mirror.centos.org stopped answering when CentOS 7 went end of life in June
# 2024. Everything still exists at vault.centos.org, frozen, which is better for
# this project's purposes than a live mirror: the package set cannot shift under
# a rebuild. Upstream's own Dockerfile performs the same redirect.
#
# # What this does not fix
#
# Packages are installed by name, not by version and not by hash. `yum install
# gcc` against a frozen vault is reproducible in practice because the vault does
# not change, and it is not reproducible by construction. Pinning every RPM by
# checksum is the honest next step and is not done yet. `docs/WHITEPAPER.md`
# lists this among the gaps rather than leaving it to be discovered.

# The `centos7` tag on quay.io, resolved to its digest. Checked to return HTTP
# 200 from the registry before being written here, because the value this
# replaces was a plausible looking digest that belonged to no image and broke
# every scheduled build with `manifest unknown`.
FROM quay.io/centos/centos@sha256:e4ca2ed0202e76be184e75fb26d14bf974193579039d5573fb2348664deef76e

# One layer, so the redirect and the install cannot get separated. A cached
# image with the redirect applied and the packages missing would fail in a way
# that looks like a network problem.
RUN set -eux; \
    sed -i \
        -e 's/^mirrorlist/#mirrorlist/' \
        -e 's/^#baseurl/baseurl/' \
        -e 's|mirror\.centos\.org|vault.centos.org|' \
        /etc/yum.repos.d/*.repo; \
    yum -y -q install \
        gcc gcc-c++ make binutils \
        glibc-devel glibc-static \
        glibc-devel.i686 glibc.i686 libgcc.i686 \
        zlib-static zlib-devel \
        autoconf automake libtool \
        diffutils file findutils which tar xz gzip bzip2 \
        python3; \
    yum clean all; \
    rm -rf /var/cache/yum

# Fixed before any build script runs, so that a Makefile which forgets to set
# them still gets the right answer. `build-blobs.sh` sets these too; having them
# here as well means a component invoked directly, by hand, in this container
# behaves the same way CI does.
ENV LC_ALL=C \
    LANG=C \
    TZ=UTC \
    ARFLAGS=Dcr

WORKDIR /build

# No CMD. This image is driven by `blobs.yml`, which passes the exact command it
# wants, and a default that quietly does something else is a trap when a run
# fails and somebody reaches for `docker run` to reproduce it.
