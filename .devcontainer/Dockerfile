FROM rust:1.98.0-bookworm

ENV PATH="/usr/local/cargo/bin:${PATH}"

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

RUN printf '%s\n' 'export PATH="/usr/local/cargo/bin:$PATH"' > /etc/profile.d/rust-toolchain.sh

WORKDIR /workspace
USER ${USERNAME}