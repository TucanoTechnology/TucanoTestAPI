# Tucano Test Rust

Rust architecture evaluation for TucanoTCM. The project preserves the file-based JSON storage model and keeps future GUI clients behind the documented HTTP API.

## Development container

Open this repository in VS Code with the Dev Containers extension and choose **Reopen in Container**. The container pins Rust `1.98.0`, installs `rustfmt` and `clippy`, runs as the non-root `vscode` user, and mounts the local `data/` directory at `/workspace/data`.

The storage location is available to the application through `TUCANO_DATA_DIR`. Keep test data and secrets out of version control; only `data/.gitkeep` is tracked.

## Local checks

From the container or a host with the pinned toolchain installed:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

The Dev Container is the first reproducibility gate. CI/CD and security scanning are planned as the next P0 tasks before service implementation.