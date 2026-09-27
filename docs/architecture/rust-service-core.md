# Rust Service Core

## Status

**Decided and implemented.** This document began as a Rust evaluation plan and is retained as the architecture record for the service core. Phases 0–2 are delivered; Phase 3 is partially delivered, as noted below.

## Decision frame

Rust was chosen for the service core because Tucano Test is a portable, file-based system exposed to untrusted HTTP input. Rust reduces memory-safety risk at compile time while retaining the project goals of inspectable JSON files, a database-free deployment, API/GUI parity, and container portability.

## Target architecture

```text
GUI (TucanoTestGUI)
    |
    | HTTP/JSON API
    v
Rust service
    |-- request validation and authentication boundary
    |-- domain services and authorization
    |-- repository traits
    |       `-- JSON filesystem repository (initial implementation)
    |-- atomic persistence and file locking
    `-- OpenAPI contract, health, metrics, and audit events
```

The GUI must use the same documented API as other clients. It should not read the storage directory directly. Storage access belongs behind a repository interface so a future storage implementation can be evaluated without coupling the GUI or HTTP layer to files.

## Security and memory-protection benefits

- Rust ownership and borrowing prevent use-after-free, double-free, and many data-race classes without a garbage collector.
- Bounds checks and safe standard-library collections reduce buffer-overrun and out-of-bounds access risk in request, attachment, and JSON handling.
- `Result` and `Option` make I/O, parsing, and missing-resource failures explicit instead of relying on unchecked exceptions or null values.
- Minimal `unsafe` should be allowed only behind reviewed, isolated modules. CI should run `cargo clippy -- -D warnings`, `cargo audit`, and an unsafe-code audit.
- Memory protection does not replace input validation, authorization, rate limiting, path confinement, or dependency review. The threat model still includes malicious JSON, oversized uploads, symlink races, corrupted files, denial of service, and leaked operational data.
- Resource limits must be explicit: request body size, attachment size, JSON nesting/depth, concurrent uploads, filesystem work, and per-request timeout.

## Planned features and changes

### Phase 0: Baseline and contract

- Capture the current endpoint behavior, status codes, JSON schemas, error envelope, request IDs, and Swagger document as compatibility fixtures.
- Define the Rust workspace and CI toolchain policy without changing production deployment.
- Decide MSRV, supported targets, release profile, container base image, and dependency update process.
- Define immutable release numbering: SemVer tags for application releases and GitHub run numbers for every main-branch build artifact.

### Phase 1: Safe core

- Add typed domain models for projects, test cases, suites, runs, and attachments using `serde`.
- Implement Draft 2020-12-compatible validation or document any validator capability differences before migration.
- Implement a repository trait and a filesystem repository rooted at one configured directory.
- Canonicalize and validate every user-controlled ID, filename, and attachment path. Reject traversal, absolute paths, separators where not allowed, symlink escapes, and unexpected file types.
- Use atomic writes through a same-directory temporary file, flush/sync where configured, restrictive permissions, and rename; define overwrite and concurrency behavior.
- Return one error envelope with stable codes, safe messages, details, and request IDs. Never serialize internal paths, stack traces, or raw filesystem errors to clients.

### Phase 2: HTTP compatibility layer

- Reimplement CRUD endpoints with typed handlers and an OpenAPI document generated or checked from the same contract.
- Preserve API/GUI parity: every GUI action must have an API equivalent and every API error must be renderable by the GUI.
- Add bounded multipart streaming for attachments instead of loading untrusted files into memory.
- Add health/readiness endpoints, structured logs, metrics, graceful shutdown, and cancellation propagation.
- Add authentication and authorization before exposing the service beyond a trusted local network. Keep audit events free of secrets and file contents.

### Phase 3: Verification and migration

- Run contract tests against Node and Rust implementations and compare status, headers, body shape, and persistence effects.
- Add property/fuzz tests for JSON parsing, IDs, path handling, error mapping, and attachment metadata.
- Run race/concurrency tests, corrupted-file recovery tests, symlink tests, oversized-input tests, and permission failure tests.
- Benchmark representative CRUD, listing, validation, and upload workloads with bounded memory and concurrency.
- Shadow or canary the Rust service with rollback to the current implementation. Migrate one resource family at a time; do not change file formats without an explicit versioning plan.

## Delivered stack

The candidates below were validated and adopted: `axum` and `tower-http` for HTTP routing, limits and tracing; `serde` and `serde_json` for typed payloads; `tokio` for asynchronous I/O; `fs2` for advisory locking; `roxmltree` for parsing JUnit XML reports in the run importer; `tempfile` for test isolation.

Still outstanding: a reviewed JSON Schema validator, `thiserror` domain errors, `tracing-subscriber` structured diagnostics, and `cargo-deny` policy checks.

## Module layout (the delivered tree)

The API is layered so that each concern has exactly one home; a layer only depends on the layers
beneath it, and nothing below the HTTP layer knows about Axum:

| Path | Role |
| --- | --- |
| `src/models.rs` | The stored documents (projects, suites, cases, runs, milestones, configurations) in their legacy JSON shapes |
| `src/storage/` | The only code that touches the filesystem: the `Repository` trait and ETag types (`mod.rs`), the `FileRepository` implementation split across `storage/fs/` (`crud.rs` documents, `attachments.rs`, `revisions.rs`, `probe.rs` diagnostics), and the path layout and confinement rules (`layout.rs`) |
| `src/domain/` | The business rules behind `TestService<R: Repository>`: `validation.rs`, identifier derivation and required fields (`resources.rs`), `composition.rs`, `duplication.rs`/`duplicate.rs`, `progress.rs`, `import.rs`, `defect.rs`, coverage and summary aggregation (`reports.rs`), error translation (`error.rs`), and the service itself in `service/` (`crud.rs`, `attachments.rs`, `audit.rs`, `cache.rs`, `history.rs`, `metadata.rs`, `reporting.rs`) |
| `src/api/` | The HTTP layer: one module per resource (`projects.rs`, `suites.rs`, `cases.rs`, `runs.rs`, `milestones.rs`, `configurations.rs`), `reports.rs` (coverage and summary), `crud.rs` (the shared handler macros), `error.rs` (the envelope), `access.rs` (the per-project guards), `auth.rs` (the bearer extractor), `guardrails.rs` (timeout, concurrency and size ceilings), `metrics.rs` (counters and the span's response recording), `redact.rs` (what may never be logged), `request_id.rs` (the span and the id's journey) |
| `src/auth/` | Authentication: `config.rs` (the settings contract and the environment-over-file precedence), `password.rs`, `token.rs`, `store.rs` (accounts, refresh tokens and grants below `auth/`), `session.rs` (the sign-in/refresh/sign-out rules), `bootstrap.rs` (the first account), and `seed.rs` (the demo accounts the seed dataset needs) |
| `src/config/` | The optional startup configuration file: its versioned strict schema, the loader, and the environment-over-file precedence rule (`mod.rs`), with the secret-file machinery in `secret.rs` |
| `src/repository.rs` | Compatibility re-export of the storage types so existing imports keep resolving |

`src/api.rs` no longer exists as a monolith: the HTTP surface lives in `src/api/`. The domain layer
is covered by unit tests that never start an HTTP server, while the integration suites drive the
router in-process. The public crate surface is unchanged — `api::router` is still the entry point
used by `main.rs` and the tests.


## Non-goals for the first prototype

- No database, distributed storage, microservice split, or direct GUI-to-filesystem access.
- No unsafe optimization before profiling demonstrates a need.
- No wholesale file-format redesign during the language evaluation.
- No claim that Rust alone provides authentication, authorization, or freedom from denial-of-service risk.

## Exit criteria

The service core is accepted once it has endpoint and file-format compatibility fixtures, passes the security and failure-mode suite, documents all deviations, demonstrates bounded resource behaviour, and can be deployed and rolled back using the existing volume-mount model.

Outstanding against these criteria: compatibility fixtures, property/fuzz testing, and benchmarking.
Authentication and authorization landed in [#130](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130),
enforced at the request-validation boundary this document describes.
