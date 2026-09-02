# Tucano Test Rust

Rust architecture evaluation for TucanoTCM. The project preserves the file-based JSON storage model and keeps future GUI clients behind the documented HTTP API.

## Development container

Open this repository in VS Code with the Dev Containers extension and choose **Reopen in Container**. The container pins Rust `1.98.0`, installs `rustfmt` and `clippy`, runs as the non-root `vscode` user, and mounts the local `data/` directory at `/workspace/data`.

The storage location is available to the application through `TUCANO_DATA_DIR`. Keep test data and secrets out of version control; only `data/.gitkeep` is tracked.

On rootless Docker hosts, bind-mounted workspace files may appear owned by container `root`; this can prevent the non-root `vscode` user from creating `Cargo.lock`. Use a Docker engine with matching UID mappings, or run the local validation command with temporary root access. The Dev Container configuration itself remains non-root.

## Local checks

From the container or a host with the pinned toolchain installed:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

The Dev Container is the first reproducibility gate. GitHub Actions runs the same checks, a release build, the Dev Container test, dependency auditing, secret scanning, and container scanning. Tags matching `v*.*.*` publish a container artifact to GHCR.

Repository contribution and agent workflow rules are documented in [AGENTS.md](AGENTS.md).

The security requirements are captured in [THREAT_MODEL.md](THREAT_MODEL.md), and the Rust migration baseline is defined in [COMPATIBILITY_CONTRACT.md](COMPATIBILITY_CONTRACT.md).