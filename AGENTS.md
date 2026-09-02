# AI Agent Rules — Tucano Test Rust

This file contains rules and guidelines for AI agents working on the Tucano Test Rust project.

---

## Core Project Philosophy

**Tucano Test is a file-based test case management system.** All test data (projects, test cases, test suites, test runs, and attachments) is stored as JSON or files on the filesystem. There is no database. The API is designed to be portable via a production container and persistent volume.

When making decisions about features, architecture, or implementation:
- Preserve the file-based approach — all CRUD operations read/write JSON files below `TUCANO_DATA_DIR`.
- Do not introduce databases, ORMs, or external persistence services.
- Keep the API stateless so replicas can share the configured persistent storage.
- Require shared storage with working advisory locks for multi-replica deployments; never use separate per-replica data volumes.
- Keep the system simple, inspectable, and movable
- **GUI and API are equal citizens** — all actions can be performed via the graphical interface or directly via API calls. Neither is secondary; both must support the same functionality
- **All API functionality must be documented in Swagger** — every endpoint, parameter, and response must be captured in the OpenAPI/Swagger specification. If it's not in Swagger, it doesn't exist

---

## Development Workflow

### Branch Strategy
When developing a new feature or carrying out a ticket:
1. **Create a new branch** for that feature: `git checkout -b feature/your-feature-name`
2. **Work on the feature branch** — do not commit directly to `main`
3. **Rebase onto main before pushing** — run `git fetch origin && git rebase origin/main` to check for conflicts and ensure your branch is up-to-date
4. **Resolve any conflicts** — if rebase reveals conflicts, resolve them locally before pushing
5. **Open a PR** when ready for review
6. **Merge via PR** after verification

### Dependencies
When adding a feature or new dependency:
- **Always check online for the latest stable version** before adding
- Use `cargo search <crate>` inside the pinned Rust build environment or check the crate's official repository
- Do not assume the version in `Cargo.toml` is current — verify before installing
- Prefer well-maintained, stable releases over bleeding-edge versions

---

## Code Standards

### File Structure
- API handlers: `src/api.rs`
- Domain models: `src/models.rs`
- Filesystem repository: `src/repository.rs`
- OpenAPI contract: `openapi.json`; interactive UI: `swagger.html`
- Tests: Rust unit/integration tests alongside the relevant module or under `tests/`
- Runtime data: `TUCANO_DATA_DIR` (mounted persistent volume, never committed)

### JSON and Schema Validation
- Validate all API payloads against the checked-in contract before persistence.
- Preserve the legacy Draft 2020-12 JSON shapes and camelCase field names.
- Validate on POST and PUT operations and reject malformed, unknown, or oversized input.

### API Design
- RESTful endpoints for each resource
- Every endpoint, parameter, request, response, and error must be represented in `openapi.json` and usable through Swagger UI at `/api-docs`.
- Consistent structured JSON error responses
- Secure filenames and prevent path traversal attacks

### Testing
- Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets --all-features`.
- Cover CRUD operations, persistence, attachments, malformed input, traversal, symlinks, concurrency, and volume behavior.
- Test the production image and Compose configuration for startup, health, persistence, and restart behavior.

---

## Docker & Deployment

### Local Run
```bash
docker compose up -d --build
```

### Ports
- API: `3000` (host) → `3000` (container)
- Swagger UI: `http://localhost:3000/api-docs`
- OpenAPI JSON: `http://localhost:3000/openapi.json`

### Volume Mount
- Compose volume: `tucano-test-data` → Container: `/data`
- Configuration: `TUCANO_DATA_DIR=/data`
- Data persists across container restarts
- Files are plain JSON — inspectable and editable on host

### Scaling
- The application container runs as an unprivileged user with a read-only root filesystem.
- Replicas share no in-memory application state.
- Scale only with a shared persistent POSIX volume and advisory-lock support.
- A local Docker volume is single-node only; use platform-provided shared storage for multi-node deployments.

---

## Security

- Validate all input against JSON schemas
- Sanitize filenames to prevent path traversal
- Use atomic writes for file operations
- Implement overwrite protection where appropriate
- Keep dependencies updated (check for security advisories)

---

## Documentation

### README.md
- **Always keep README.md up to date** — it must reflect the latest description, build process, goals, and feature set
- Update README whenever you:
  - Add new features or endpoints
  - Change the build or deployment process
  - Modify the project structure
  - Add new dependencies or requirements
- README should include:
  - Project overview and goals
  - Quick start guide (how to build and run)
  - Feature list with brief descriptions
  - API documentation link (Swagger UI)
  - Configuration options
  - Testing instructions

### Code Documentation
- Update README.md for user-facing changes.
- Keep `openapi.json` and Swagger UI in sync with routes.
- Document changes to file formats, compatibility behavior, persistence, or deployment.
- Keep architecture, security, and compatibility documents versioned with the code.

---

## Git & Version Control

- **Protect the main branch** — never commit directly to `main`. All changes must go through pull requests
- Use conventional commit messages: `feat:`, `fix:`, `docs:`, `chore:`, `test:`
- Keep commits focused — one logical change per commit
- Reference issue/ticket numbers in commit messages when applicable
- **NEVER commit sensitive data** — this includes:
  - API keys and tokens (GitHub, AWS, etc.)
  - Passwords and secrets
  - Private keys and certificates
  - Database credentials
  - Any environment-specific configuration
  - Use environment variables or `.env` files (added to `.gitignore`) instead
- **Require CI to pass** — PRs must pass all CI checks before merging
- **Require up-to-date branches** — branches must be up-to-date with main before merging

## Release Numbering Policy

- Application releases use Semantic Versioning in `Cargo.toml` and immutable `vMAJOR.MINOR.PATCH` Git tags.
- Every push to `main` receives an immutable GitHub Actions run number and publishes a container tag in the form `build-<run number>`.
- A SemVer release publishes both its `vMAJOR.MINOR.PATCH` tag and its build-number tag; the commit SHA is the audit identity.
- Never reuse or overwrite a release tag or build number. Pull requests may build artifacts for validation but do not publish releases.
- Release artifacts must be built from protected `main` or protected release tags and must retain the JSON volume-mount model; no database or external persistence service may be introduced.

---

## Project Board Management

**Project board:** https://github.com/orgs/TucanoTechnology/projects/3/views/1

### Ticket Requirements
Every ticket on the project board must have:

1. **Description** — clear explanation of what needs to be done and why
2. **Definition of Done** — specific criteria that must be met for the ticket to be considered complete
3. **Labels/Tags** — appropriate labels (e.g., `enhancement`, `documentation`, `security`, `bug`, `testing`)
4. **Complexity Estimate** — effort estimation (e.g., `small`, `medium`, `large` or story points)
5. **Priority** — one of:
   - **Security** — highest priority, security-related issues
   - **Bug fix** — fixing broken functionality
   - **Feature** — new functionality or enhancements

### Workflow
- **Every change must have a ticket** — if you're working on something and no ticket exists, create one first
- **Update tickets as you work** — add comments documenting progress, decisions, and blockers
- **Mark tickets complete** — when done, add a final comment summarizing what was accomplished and close the ticket
- **Link PRs to tickets** — reference ticket numbers in PR descriptions and commit messages

### Ticket Template
When creating a new ticket, include:
```
**Description:**
What needs to be done and why.

**Definition of Done:**
- [ ] Criterion 1
- [ ] Criterion 2
- [ ] Tests pass
- [ ] Documentation updated
- [ ] PR reviewed and merged

**Labels:** [appropriate labels]
**Complexity:** [small/medium/large]
**Priority:** [security/bug fix/feature]
```

---

## Questions?

If these rules conflict with a ticket or requirement:
1. Prioritize the file-based philosophy
2. Clarify with the project maintainer
3. Document any exceptions in the PR description
