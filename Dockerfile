FROM rust:1.98.0-bookworm AS builder

ARG BUILD_NUMBER=local

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY openapi.json swagger.html ./
RUN cargo build --release

FROM ubuntu:26.04

ARG BUILD_NUMBER=local

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
