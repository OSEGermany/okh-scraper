# syntax=docker/dockerfile:1
# NOTE Lint this file with https://hadolint.github.io/hadolint/

# SPDX-FileCopyrightText: 2022-2025 Robin Vobruba <hoijui.quaero@gmail.com>
#
# SPDX-License-Identifier: Unlicense

# First, compile in the rust container
FROM rust:1.82-bookworm AS rust-builder

WORKDIR /usr/src/app

RUN apt-get update && \
    apt-get install -y \
        cmake \
        && \
    rm -rf /var/lib/apt/lists/*

COPY ["Cargo.*", "."]
COPY ["src", "./src"]
RUN cargo install --locked --path .

# Then use a minimal container
# and only copy over the required files
# generated in the previous container(s).
FROM bitnami/minideb:bookworm

RUN install_packages \
    ca-certificates

COPY --from=rust-builder /usr/local/cargo/bin/* /usr/local/bin/

# NOTE Labels and annotations are added by CI (outside this Dockerfile);
#      see `.github/workflows/docker.yml`.
#      This also means they will not be available in local builds.

WORKDIR /home/okh-scraper/

ENTRYPOINT ["okh-scraper"]
CMD []
