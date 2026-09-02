FROM rust:1.98.0-bookworm

ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=1000

RUN groupadd --gid ${USER_GID} ${USERNAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} -m -s /bin/bash ${USERNAME} \
    && apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add rustfmt clippy \
    && mkdir -p /workspace/data \
    && chown -R ${USERNAME}:${USERNAME} /workspace

WORKDIR /workspace
USER ${USERNAME}