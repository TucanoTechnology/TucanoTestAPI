# AI Agent Rules — Tucano Test API

Rules and guidelines for AI agents working on the Tucano Test API.

## Shared rules

Organisation-wide rules live in [`AgentRules/`](AgentRules/) and apply to every repository.
Read them before making changes. This file records only what is specific to this repository.

| Area | Rules |
| --- | --- |
| Branching | [`AgentRules/coding/branch-strategy.md`](AgentRules/coding/branch-strategy.md) |
| Git usage | [`AgentRules/coding/git-usage-policy.md`](AgentRules/coding/git-usage-policy.md) |
| Dependencies | [`AgentRules/coding/dependencies.md`](AgentRules/coding/dependencies.md) |
| API design | [`AgentRules/coding/api-design.md`](AgentRules/coding/api-design.md) |
| JSON and schema validation | [`AgentRules/coding/json-and-schema-validation.md`](AgentRules/coding/json-and-schema-validation.md) |
| Code review | [`AgentRules/coding/code-review-best-practices.md`](AgentRules/coding/code-review-best-practices.md) |
| Secret protection | [`AgentRules/security/secret-protection.md`](AgentRules/security/secret-protection.md) |
| Security and commits | [`AgentRules/security/security-and-commit-rules.md`](AgentRules/security/security-and-commit-rules.md) |
| Ticket management | [`AgentRules/project-management/ticket-management-policy.md`](AgentRules/project-management/ticket-management-policy.md) |
| Ticket updates | [`AgentRules/project-management/ticket-update-policy.md`](AgentRules/project-management/ticket-update-policy.md) |
| Ticket template | [`AgentRules/project-management/ticket-template.md`](AgentRules/project-management/ticket-template.md) |
| Workflow | [`AgentRules/project-management/workflow.md`](AgentRules/project-management/workflow.md) |
| Unit tests | [`AgentRules/test/unit.md`](AgentRules/test/unit.md) |
| Contract tests | [`AgentRules/test/contract.md`](AgentRules/test/contract.md) |
| Performance tests | [`AgentRules/test/performance.md`](AgentRules/test/performance.md) |
| Playwright tests | [`AgentRules/test/playwright.md`](AgentRules/test/playwright.md) |
| Conflicts and exceptions | [`AgentRules/generic/questions.md`](AgentRules/generic/questions.md) |

Related repository: [TucanoTestGUI](https://github.com/TucanoTechnology/TucanoTestGUI).

---

## Core Project Philosophy

**Tucano Test is a file-based test case management system.** All test data (projects, test cases,
test suites, test runs, and attachments) is stored as JSON or files on the filesystem. There is no
database. The API is designed to be portable via a production container and persistent volume.

When making decisions about features, architecture, or implementation:

- Preserve the file-based approach — all CRUD operations read/write JSON files below `TUCANO_DATA_DIR`.
- Do not introduce databases, ORMs, or external persistence services.
- Keep the API stateless so replicas can share the configured persistent storage.
- Require shared storage with working advisory locks for multi-replica deployments; never use
  separate per-replica data volumes.
- Keep the system simple, inspectable, and movable.
- **GUI and API are equal citizens** — all actions can be performed via the graphical interface or
  directly via API calls. Neither is secondary; both must support the same functionality.
- **All API functionality must be documented in Swagger** — if it is not in `openapi.json`, it does
  not exist.

---

## Code Standards

### File structure

- API handlers: `src/api.rs`
- Domain models: `src/models.rs`
- Filesystem repository: `src/repository.rs`
- Library target: `src/lib.rs`; binary entry point: `src/main.rs`
- OpenAPI contract: `openapi.json`; interactive UI: `swagger.html`
- Unit tests: alongside the relevant module
- Integration tests: `tests/`, one suite per API area, with shared helpers in `tests/common/mod.rs`
- Runtime data: `TUCANO_DATA_DIR` (mounted persistent volume, never committed)

### Project-specific data rules

- Preserve the legacy Draft 2020-12 JSON shapes and camelCase field names.
- The legacy schemas set `additionalProperties: false`; adding a field is a breaking change and
  requires an explicit versioning plan in `COMPATIBILITY_CONTRACT.md` before implementation.
- Test run results belong to the run, keyed per test case, so a case or suite may appear in
  several runs with different outcomes.

### Dependencies

Use `cargo search <crate>` inside the pinned Rust environment to confirm the latest stable version.

---

## Testing

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Cover CRUD operations, persistence, attachments, malformed input, traversal, symlinks,
concurrency, and volume behaviour. Test the production image and Compose configuration for
startup, health, persistence, and restart behaviour.

---

## Docker & Deployment

### Local run

```bash
docker compose up -d --build
```

### Ports

- API: `3000` (host) → `3000` (container)
- Swagger UI: `http://localhost:3000/api-docs`
- OpenAPI JSON: `http://localhost:3000/openapi.json`

### Volume mount

- Compose volume: `tucano-test-data` → Container: `/data`
- Configuration: `TUCANO_DATA_DIR=/data`
- Data persists across container restarts
- Files are plain JSON — inspectable and editable on host

### Scaling

- The application container runs as an unprivileged user with a read-only root filesystem.
- Replicas share no in-memory application state.
- Scale only with a shared persistent POSIX volume and advisory-lock support.
- A local Docker volume is single-node only; use platform-provided shared storage for multi-node
  deployments.

---

## Storage Security

Beyond the shared security rules:

- Sanitise every user-supplied identifier and filename to prevent path traversal.
- Use atomic writes (same-directory temporary file, flush, rename) for all persistence.
- Apply restrictive file permissions and explicit overwrite behaviour.
- Never return internal paths, stack traces, or raw filesystem errors to clients.

---

## Documentation

- **Always keep README.md up to date** — it must reflect the latest description, build process,
  goals, and feature set.
- Keep `openapi.json` and Swagger UI in sync with routes.
- Document changes to file formats, compatibility behaviour, persistence, or deployment.
- Keep architecture, security, and compatibility documents versioned with the code.

---

## Release Numbering Policy

- Application releases use Semantic Versioning in `Cargo.toml` and immutable `vMAJOR.MINOR.PATCH`
  Git tags.
- Every push to `main` receives an immutable GitHub Actions run number and publishes a container
  tag in the form `build-<run number>`.
- A SemVer release publishes both its `vMAJOR.MINOR.PATCH` tag and its build-number tag; the commit
  SHA is the audit identity.
- Never reuse or overwrite a release tag or build number. Pull requests may build artifacts for
  validation but do not publish releases.
- Release artifacts must be built from protected branches or protected release tags and must retain
  the JSON volume-mount model.

---

## Project Board

**Project board:** https://github.com/orgs/TucanoTechnology/projects/3/views/1

Ticket creation, update and closure rules are defined in `AgentRules/project-management/`.
