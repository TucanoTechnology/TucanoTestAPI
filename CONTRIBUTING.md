# Contributing to TucanoTestAPI

Thank you for your interest in contributing. This guide covers local setup,
testing, and the pull-request process.

## Prerequisites

- **Rust 1.98+** — the project pins its toolchain in `rust-toolchain.toml`.
  Install via [rustup](https://rustup.rs/); `rustup` reads the file and
  installs the correct version automatically.
- **Node.js 18+** — required by the CI helper scripts (`scripts/*.mjs`) that
  validate the OpenAPI contract and documentation links.
- **Docker** — required to run the container image locally and to execute the
  actionlint and gitleaks containers used in CI.

## Local Development

### Build and run

```bash
# Start the API with a local data directory
TUCANO_DATA_DIR=./data cargo run

# Or use Docker Compose for a containerised stack
docker compose up -d --build
```

The API listens on port `3000` by default. Swagger UI is at
<http://localhost:3000/api-docs>.

### Seed demo data

```bash
cargo run -- seed-auth --username admin --password <password>
```

### Run the full CI suite locally

Every CI job must pass locally before committing:

```bash
# 1. Format check
cargo fmt --all -- --check

# 2. Lint
cargo clippy --all-targets --all-features -- -D warnings

# 3. Documentation (catches broken intra-doc links)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items

# 4. Tests
cargo test --all-targets --all-features

# 5. Release build
cargo build --release

# 6. GitHub Actions syntax (requires Docker)
docker run --rm -v "$PWD:/repo:ro" --workdir /repo rhysd/actionlint:1.7.12 -color

# 7. API coverage matrix
node scripts/check-matrix.mjs

# 8. Documentation links
node scripts/check-docs-links.mjs
```

## Code Standards

### Architecture

The crate follows a layered design:

| Layer | Directory | Responsibility |
|-------|-----------|----------------|
| HTTP | `src/api/` | Routes, request parsing, response shaping |
| Domain | `src/domain/` | Business rules, validation, composition |
| Storage | `src/storage/` | Filesystem persistence behind the `Repository` trait |
| Models | `src/models.rs` | Serde structs for the JSON documents |
| Auth | `src/auth/` | Authentication and project authorisation |
| Config | `src/config/` | Configuration file and precedence resolution |

Each layer depends only on the one beneath it. A handler does three things:
pull values from the request, call `TestService`, and shape the response.

### Conventions

- **One module per resource** in `src/api/` — projects, suites, runs, cases,
  milestones, configurations, reports.
- **CRUD macros** in `src/api/crud.rs` generate the five standard handlers; a
  resource module adds only what is unique to it.
- **`deny_unknown_fields`** on every serde struct — a typo in a key name is a
  startup or request error, not a silently ignored field.
- **Doc comments** on every public item, with `# Errors` sections where
  applicable.
- **No `unwrap()`** in production code — all error paths are handled
  explicitly.
- **`forbid(unsafe_code)`** at the crate level.

### Testing

- **Unit tests** live alongside the code in `#[cfg(test)] mod tests` blocks.
- **Integration tests** live in `tests/`, one file per API area, with shared
  helpers in `tests/common/mod.rs`.
- Cover CRUD operations, persistence, attachments, malformed input, path
  traversal, symlinks, concurrency, and volume behaviour.

## Pull Request Process

1. **Create a feature branch** from the default branch.
2. **Run the full CI suite** locally (see above) and fix any failures.
3. **Commit** with a clear, conventional commit message.
4. **Push** and open a pull request against the default branch.
5. **Assign** the pull request to a project maintainer for review.
6. **Address** review comments and push fixes.
7. Once every required CI job passes and a maintainer approves, the PR is
   merged.

## Documentation

- Keep `README.md` up to date with the latest description, build process, and
  feature set.
- Keep `openapi.json` and `swagger.html` in sync with routes — if it is not in
  the OpenAPI spec, it does not exist.
- Architecture decisions belong in `docs/architecture/`, compatibility rules
  in `docs/contracts/`, and security documentation in `docs/security/`.
- Wiki pages live in `docs/wiki/` and are mirrored to the GitHub Wiki by CI.

## Security

- Never commit secrets, API keys, or credentials.
- Sanitise every user-supplied identifier to prevent path traversal.
- Use atomic writes for all persistence.
- Never return internal paths, stack traces, or raw filesystem errors to
  clients.
- Report security issues privately — see
  `docs/security/threat-model.md` for the trust boundary.
