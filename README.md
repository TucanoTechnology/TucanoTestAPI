# Tucano Test Rust

Rust architecture evaluation for TucanoTCM. The project preserves the file-based JSON storage model and keeps future GUI clients behind the documented HTTP API.

## Application container

Build and run the production API with Docker Compose:

```sh
docker compose up -d --build
```

The API listens on port `3000`, runs as an unprivileged user, and stores inspectable JSON and attachments in the persistent `tucano-test-data` volume mounted at `/data`. The storage location is configurable through `TUCANO_DATA_DIR`. Keep test data and secrets out of version control; only `data/.gitkeep` is tracked.

Interactive Swagger UI is available at `http://localhost:3000/api-docs`; the raw OpenAPI document is at `http://localhost:3000/openapi.json`.

The API process is stateless: replicas do not keep sessions or in-memory records. Horizontal scaling requires a shared persistent POSIX volume mounted at the same `TUCANO_DATA_DIR` for every replica. Repository mutations use an advisory lock file and atomic same-directory renames. A local Docker volume is suitable for one node; multi-node deployments must provide shared storage with working advisory locks. Do not use separate per-replica local volumes, or data will diverge.

## Local checks

From the container or a host with the pinned toolchain installed:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

GitHub Actions runs formatting, Clippy, tests, a release build, dependency auditing, secret scanning, and production container scanning. Tags matching `v*.*.*` publish a container artifact to GHCR.

Repository contribution and agent workflow rules are documented in [AGENTS.md](AGENTS.md).

The security requirements are captured in [THREAT_MODEL.md](THREAT_MODEL.md), and the Rust migration baseline is defined in [COMPATIBILITY_CONTRACT.md](COMPATIBILITY_CONTRACT.md).