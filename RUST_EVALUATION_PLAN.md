# TucanoTCM Rust Evaluation Plan

## Decision frame

Evaluate Rust for the service core because TucanoTCM is intended to be a portable, file-based system exposed to untrusted HTTP input. Rust can reduce memory-safety risk at compile time while retaining the project goals of inspectable JSON files, a database-free deployment, API/GUI parity, and container portability.

This is an evaluation, not a rewrite commitment. The existing API remains the behavioral reference until the Rust service passes compatibility, security, and operational gates.

## Target architecture

```text
GUI (future)
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

## Suggested Rust stack

- `axum` and `tower-http` for HTTP routing, limits, tracing, and request IDs.
- `serde`, `serde_json`, and a reviewed JSON Schema validator for typed payloads.
- `thiserror` for domain errors and `anyhow` only at application boundaries.
- `tokio` for asynchronous I/O and cancellation.
- `tracing`/`tracing-subscriber` for structured diagnostics.
- `tempfile` plus platform-aware filesystem operations for atomic persistence.
- `cargo-deny` or equivalent policy checks, `cargo-audit`, and an SBOM in release CI.

These are candidates to validate during Phase 0, not dependencies to add before the compatibility and security requirements are agreed.

## Non-goals for the first prototype

- No database, distributed storage, microservice split, or direct GUI-to-filesystem access.
- No unsafe optimization before profiling demonstrates a need.
- No wholesale file-format redesign during the language evaluation.
- No claim that Rust alone provides authentication, authorization, or freedom from denial-of-service risk.

## Exit criteria

The evaluation is successful when the Rust prototype has endpoint and file-format compatibility fixtures, passes the security and failure-mode suite, documents all deviations, demonstrates bounded resource behavior, and can be deployed and rolled back using the existing volume-mount model. Only then should a full migration be proposed.