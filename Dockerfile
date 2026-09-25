# Tag `rust:1.98.0-slim-trixie` resolved to its manifest-list digest, matching the
# runtime stage below, so the audited image is the image this revision builds.
FROM rust:1.98.0-slim-trixie@sha256:17d1ba895198f9934c6314ec5346a0d5115372f3243390c3d731e242f35c2f27 AS builder

ARG BUILD_NUMBER=local

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches
COPY openapi.json swagger.html ./
RUN cargo build --release

# The base image is pulled by digest rather than by tag so the cache key is the
# published image instead of whatever happens to be cached locally.
FROM debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f

ARG BUILD_NUMBER=local

# CACHE_BUSTER is deliberately bumped whenever the scan reports a fixed CVE.
# The apt index is fetched inside this layer, so without a change to this
# argument Docker reuses the cached layer, `apt-get upgrade` never sees patch
# releases published after the layer was written, and the image keeps reporting
# fixes it cannot reach. Changing it guarantees a fresh index, which is what
# makes "rebuild to pick up security updates" actually work.
ARG CACHE_BUSTER=2026-09-14

RUN apt-get update \
    && apt-get upgrade --yes \
    && apt-get install --no-install-recommends --yes ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin tucano \
    && mkdir -p /data \
    && chown tucano:tucano /data

COPY --from=builder /build/target/release/tucano-test /usr/local/bin/tucano-test

LABEL org.opencontainers.image.version="${BUILD_NUMBER}"

ENV TUCANO_DATA_DIR=/data
ENV PORT=3000
EXPOSE 3000
VOLUME ["/data"]
USER tucano
ENTRYPOINT ["/usr/local/bin/tucano-test"]
