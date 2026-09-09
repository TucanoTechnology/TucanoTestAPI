# Tucano Test Rust

Rust architecture evaluation for TucanoTCM. The project preserves the file-based JSON storage model and keeps future GUI clients behind the documented HTTP API.

## Prerequisites

To build and run the project manually you need:

| Requirement | Version | Purpose |
| --- | --- | --- |
| Rust toolchain | `1.98.0` (pinned by `rust-toolchain.toml`) | Build and test the API |
| `rustfmt` and `clippy` components | Bundled with the pinned toolchain | Formatting and lint gates |
| Docker Engine | 24 or newer | Build and run the production container |
| Docker Compose plugin | v2 | Local deployment via `docker compose` |
| `actionlint` (optional) | `1.7.12` | Validate workflow files locally |

Install the toolchain with [rustup](https://rustup.rs); `rust-toolchain.toml` pins the exact version automatically:

```sh
rustup show
cargo build --release
```

If you prefer not to install Rust locally, every command below also runs inside the pinned image:

```sh
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.98.0-bookworm cargo test --all-targets --all-features
```

## Editor setup

Recommended VS Code extensions are listed in `.vscode/extensions.json` and VS Code will offer to install them when the workspace is opened:

- `rust-lang.rust-analyzer` — Rust language support
- `github.vscode-github-actions` — workflow authoring and validation
- `redhat.vscode-yaml` — YAML schema validation
- `tamasfe.even-better-toml` — `Cargo.toml` support

`.vscode/settings.json` maps `.github/workflows/*.yml` to the SchemaStore GitHub Actions schema so workflow files validate correctly.

The GitHub Actions extension may report `Context access might be invalid: GITHUB_TOKEN` on `.github/workflows/release.yml`. This is a known false positive: `GITHUB_TOKEN` is injected automatically by GitHub Actions and is not a user-defined repository secret, so the extension cannot resolve it while signed out. The workflows are validated in CI with `actionlint`, which reports no issues.

## Application container

Build and run the production API with Docker Compose:

```sh
docker compose up -d --build
```

The API listens on port `3000`, runs as an unprivileged user, and stores inspectable JSON and attachments in the persistent `tucano-test-data` volume mounted at `/data`. The storage location is configurable through `TUCANO_DATA_DIR`. Keep test data and secrets out of version control; only `data/.gitkeep` is tracked.

Interactive Swagger UI is available at `http://localhost:3000/api-docs`; the raw OpenAPI document is at `http://localhost:3000/openapi.json`.

The API process is stateless: replicas do not keep sessions or in-memory records. Horizontal scaling requires a shared persistent POSIX volume mounted at the same `TUCANO_DATA_DIR` for every replica. Repository mutations use an advisory lock file and atomic same-directory renames. A local Docker volume is suitable for one node; multi-node deployments must provide shared storage with working advisory locks. Do not use separate per-replica local volumes, or data will diverge.

## Test Data Seeding and Cleanup

Helper scripts are provided in `scripts/` to quickly populate or wipe sample test data against a running API instance (e.g. for GUI testing or manual verification):

### Prerequisites

Node.js 18+ (uses native `fetch` and ES modules).

### Seeding Data

Populates realistic test cases (with attachments), test suites, projects, test runs (with execution results), and milestones:

```sh
# Auto-detects local API (defaulting to http://localhost:3100, http://localhost:8080/api, or http://localhost:3000)
./scripts/seed-data.mjs

# Or provide a custom API base URL:
./scripts/seed-data.mjs http://localhost:3000
```

### Clearing Data

Wipes all test cases, test suites, projects, test runs, and milestones from the API:

```sh
./scripts/clear-data.mjs

# Or provide a custom API base URL:
./scripts/clear-data.mjs http://localhost:3000
```

## Release numbering

Application releases use Semantic Versioning. Update the Cargo package version and create a protected `vMAJOR.MINOR.PATCH` tag for a release; release tags are immutable and must never be reused. Every push to `main` also publishes an immutable GHCR image tagged `build-<GitHub run number>`. Tagged releases publish both the SemVer tag and their build number, while the commit SHA remains the audit identity. Pull requests build and test without publishing release artifacts.

## Local checks

From the container or a host with the pinned toolchain installed:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Validate the workflow files with the same linter CI uses:

```sh
docker run --rm -v "$PWD:/repo:ro" --workdir /repo rhysd/actionlint:1.7.12 -color
```

## Testing

Tests are split into two layers and both run in CI on every push and pull request:

- **Unit tests** live beside the code in `src/models.rs` and `src/repository.rs`. They cover legacy JSON compatibility, required and unknown field handling, atomic writes, path confinement, attachment storage, concurrent writers, and file permissions.
- **Integration tests** live in `tests/` and exercise the HTTP surface in-process through the Axum router against a temporary data directory. Each API area has its own suite:

| Suite | Covers |
| --- | --- |
| `tests/service.rs` | Health, OpenAPI document, Swagger UI, malformed bodies, traversal rejection, persistence across restarts |
| `tests/projects.rs` | Project CRUD, validation, conflicts, error envelopes |
| `tests/suites.rs` | Test suite CRUD, validation, conflicts, missing resources |
| `tests/runs.rs` | Test run CRUD, validation, conflicts, missing resources |
| `tests/cases.rs` | Test case CRUD, required fields, conflicts, missing resources |
| `tests/attachments.rs` | Upload, download, delete, content types, removal with the parent test case |

Shared request builders and assertions live in `tests/common/mod.rs`. Cargo compiles only top-level files in `tests/` as test binaries, so a subdirectory module is shared across suites without running as one itself.

Run everything, a single layer, or one suite:

```sh
cargo test --all-targets --all-features   # unit + integration
cargo test --lib                          # unit tests only
cargo test --test attachments             # a single API suite
```

The crate exposes a library target (`src/lib.rs`) alongside the binary so integration tests can import `tucano_test::api` and drive the router directly, without binding a network port.

GitHub Actions runs workflow linting, formatting, Clippy, unit and integration tests, a release build, dependency auditing, secret scanning, and production container scanning. Main-branch builds and SemVer tags publish numbered container artifacts to GHCR.

Repository contribution and agent workflow rules are documented in [AGENTS.md](AGENTS.md).

## Documentation

| Document | Purpose |
| --- | --- |
| [docs/architecture/rust-service-core.md](docs/architecture/rust-service-core.md) | Why Rust, the layered service design, and delivery status |
| [docs/contracts/api-compatibility.md](docs/contracts/api-compatibility.md) | File-format and endpoint compatibility rules against the legacy implementation |
| [docs/security/threat-model.md](docs/security/threat-model.md) | Trust boundaries, abuse cases, and security invariants |
| [AgentRules/](AgentRules/) | Organisation-wide engineering and process rules |
