# Tucano Test Rust

Rust architecture evaluation for TucanoTCM. The project preserves the file-based JSON storage model and keeps future GUI clients behind the documented HTTP API.

## Storage concept

Tucano Test is a **file-based test case management system**: there is no database. All state is
kept as folders and JSON files on the filesystem, and everything is managed through the HTTP API —
create, read, update, delete, and duplicate. The GUI and the API are equal citizens: every GUI
action has an API equivalent, and no client ever touches the storage directory directly.

The folder layout mirrors the conceptual organisation of the domain. Each entity is a folder that
contains a JSON file with its details plus any supplementary files that belong to it:

```text
TUCANO_DATA_DIR/
├── projects/
│   └── <project>/
│       ├── project.json                 project details
│       ├── <test case>/                 case data directly in the project
│       │   ├── test-case.json           case details, steps, expected results
│       │   └── <attachments>
│       └── <test suite>/
│           ├── suite.json               suite details
│           └── <test case>/
│               ├── test-case.json
│               └── <attachments>
├── test_runs/<id>.json                  point-in-time runs and their results
├── milestones/<id>.json                 milestone details
└── configurations/<id>.json             environment configurations
```

A parent marker keeps the legacy document shape with an empty child array (`project.json` stores
`testSuites: []`, `suite.json` stores `testCases: []`); membership is the folders themselves.
Reads assemble the child documents from the tree, so `GET /projects/{id}` returns its suites (each
recursively assembled) plus an optional response-only `testCases` field of directly owned cases, and
`GET /test_suites/{id}` returns its member cases. Because membership lives in the folders, the
stored markers can never contradict the tree.

Every document the API writes also carries the identity field its model requires. When a create
body omits the `projectId`, `suiteId`, `testRunId`, `milestoneId`, or `configId` that its id is
derived from, the stored document records it, and a test run stored without a `timestamp` records
when it was written. A value the body did supply is never overwritten, so a name-only create such
as `POST /test_runs {"name": "nightly"}` stores a run that reads back as its typed model instead of
one that fails to load. The rules are recorded in
[docs/contracts/api-compatibility.md](docs/contracts/api-compatibility.md).

The conceptual hierarchy, as distinct from the exact on-disk encoding:

- **Project** — the container for the work being tested. Contains multiple test cases and multiple
  test suites; a test case may therefore live directly inside a project, without belonging to a
  suite.
- **Test Suite** — a reusable collection of test cases with its own suite-level data. Contains
  multiple test cases. A suite always lives inside a project.
- **Test Case** — a single test: its details, steps, expected results, and supplementary files
  (for example attachments). Lives inside a project or inside a suite.

Three properties follow from the concept and are binding on any implementation:

1. **The file structure represents the conceptual organisation.** Nested entities are stored under
   their parent rather than in sibling directories with duplicated copies. The on-disk tree must
   read the same way the domain model reads: project → (suite →) test case.
2. **A test run is a point-in-time execution.** A run captures the set of test cases and test suites
   it executed plus the results recorded for that run. The same test case or suite can appear in
   multiple test runs with different results, and later edits to a case or suite never rewrite what
   a finished run recorded.
3. **Supplementary files live with their entity.** Attachments are stored inside their test case
   folder; run results are stored inside their test run folder (or as files under it).

Milestones and test runs must never go silently stale when the source cases or suites they refer to
change: they either carry their own snapshot at inclusion time or record the history of the runs
they were included in with that run's results.

Three API semantics follow from this concept and apply to every composition request:

- **A real parent is required at creation.** A test suite is created inside its project and a test
  case inside its project or a test suite; nothing is created in a standalone top-level pool. The
  on-disk tree mirrors these homes: a suite folder lives under its project and a case folder under
  its project or its suite. The creation endpoints are parent-scoped —
  `POST /projects/{id}/test_suites`, `POST /projects/{id}/test_cases`, and
  `POST /test_suites/{id}/test_cases`; the retired flat `POST /test_suites` and `POST /test_cases`
  answer `400 Bad Request` naming their replacement. Reads remain global — listing and retrieval
  search the whole tree, so cases and suites are always findable regardless of home.
- **Inclusion is copy by default and move opt-in.** Adding an existing case or suite to another
  parent accepts `"mode": "copy" | "move"` and defaults to `copy`: `copy` duplicates the entity
  under the target parent (duplicate-on-include) while the source keeps its home and both copies
  are editable independently; `move` relocates the entity so the target parent becomes its only
  home. Test runs always copy at inclusion — they snapshot the selected cases and suites and never
  own them.
- **Identifiers are unique where they live.** A project id is globally unique, a suite id is unique
  within its project, and a case id is unique within its parent. Copy-on-include may therefore place
  the same id under several parents; a document-level route (`GET`/`PUT`/`DELETE /test_cases/{id}`,
  attachments, duplicate) operates on the one occurrence when it is unique and answers
  `409 Conflict`, naming the parent-scoped routes, when it is ambiguous. Listing routes never fail on
  duplicates; they de-duplicate.

This concept is enforced for agent work in [AGENTS.md](AGENTS.md).

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

### Errors and request limits

Every rejection the application raises answers the stable envelope
`{"error": {"code": "…", "message": "…"}}`. Published codes are `invalid_id`, `invalid_request`,
`invalid_status`, `invalid_multipart`, `missing_file`, `not_found`, `conflict`, and `storage_error`.
`openapi.json` names, per operation, the codes that operation can return.

Two answers do not use the envelope, because they come from the router or an extractor rather than from a
handler:

- A request body larger than 50 MiB is refused by the router's size limit with `413` and the plain-text body
  `length limit exceeded`. The limit applies to every route, so it is checked before any handler runs.
- A body the multipart extractor cannot frame — an upload sent as JSON, or without a boundary — is answered
  with `400 text/plain`. Once the framing parses, upload rejections use the envelope.

The full reconciliation of the documented error contract and schema strictness is recorded in
[docs/contracts/api-compatibility.md](docs/contracts/api-compatibility.md).

The API process is stateless: replicas do not keep sessions or in-memory records. Horizontal scaling requires a shared persistent POSIX volume mounted at the same `TUCANO_DATA_DIR` for every replica. Repository mutations use an advisory lock file and atomic same-directory renames. A local Docker volume is suitable for one node; multi-node deployments must provide shared storage with working advisory locks. Do not use separate per-replica local volumes, or data will diverge.

## Test Data Cleanup

Helper scripts are provided in `scripts/` to wipe sample test data against a running API instance:

### Prerequisites

Node.js 18+ (uses native `fetch` and ES modules).

### Clearing Data

Wipes all test cases, test suites, projects, test runs, and milestones from the API:

```sh
# From repository root:
node scripts/clear-data.mjs
# or (if executable permissions are set):
./scripts/clear-data.mjs

# From within the scripts/ folder:
node clear-data.mjs

# Provide a custom API base URL if needed:
node scripts/clear-data.mjs http://localhost:3000
```

## Release numbering

Application releases use Semantic Versioning. Update the Cargo package version and create a protected `vMAJOR.MINOR.PATCH` tag for a release; release tags are immutable and must never be reused. Every push to `main` also publishes an immutable GHCR image tagged `build-<GitHub run number>`. Tagged releases publish both the SemVer tag and their build number, while the commit SHA remains the audit identity. Pull requests build and test without publishing release artifacts.

## Module layout

The API is layered so that each concern has exactly one home; a layer only depends on the layers
beneath it, and nothing below the HTTP layer knows about Axum:

| Path | Role |
| --- | --- |
| `src/models.rs` | The stored documents (projects, suites, cases, runs, milestones, configurations) in their legacy JSON shapes |
| `src/storage/` | The only code that touches the filesystem: the `Repository` trait, its `FileRepository` implementation, the path layout, and path confinement |
| `src/domain/` | The business rules — validation, composition, duplication, milestone progress — behind `TestService<R: Repository>` |
| `src/api/` | The HTTP layer: one module per resource, plus `crud.rs` (the shared handler macros) and `error.rs` (the error envelope) |
| `src/repository.rs` | Compatibility re-export of the storage types so existing imports keep resolving |

`src/api.rs` no longer exists as a monolith: the HTTP surface lives in `src/api/`. The domain layer
is covered by unit tests that never start an HTTP server, while the integration suites drive the
router in-process. The public crate surface is unchanged — `api::router` is still the entry point
used by `main.rs` and the tests.

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

- **Unit tests** live beside the code in `src/models.rs`, `src/storage/`, and `src/domain/`. They cover legacy JSON compatibility, payload validation, composition and duplication rules, milestone progress, atomic writes, path confinement, attachment storage, concurrent writers, and file permissions.
- **Integration tests** live in `tests/` and exercise the HTTP surface in-process through the Axum router against a temporary data directory. Each API area has its own suite:

| Suite | Covers |
| --- | --- |
| `tests/service.rs` | Health, OpenAPI document and its error contract, Swagger UI, malformed bodies, traversal rejection, the identifier and size-limit error answers, the on-disk tree layout, persistence across restarts |
| `tests/projects.rs` | Project CRUD, validation, conflicts, error envelopes |
| `tests/suites.rs` | Test suite CRUD, parent-scoped creation, copy/move composition, ambiguity conflicts, missing resources |
| `tests/runs.rs` | Test run CRUD, validation, conflicts, missing resources, JUnit XML and JSON result import, and listing, linking and unlinking the defect links a result carries |
| `tests/cases.rs` | Test case CRUD, required fields, parent-scoped creation, copy/move composition, conflicts, missing resources |
| `tests/milestones.rs` | Milestone CRUD, validation, conflicts, duplication, and progress derived from the referenced runs |
| `tests/configurations.rs` | Configuration CRUD, validation, conflicts, missing resources, restart persistence, and use by a run |
| `tests/attachments.rs` | Upload, download, delete, content types, removal with the parent test case |
| `tests/tags.rs` | The `tags` array on projects, suites, cases and runs, the shared `?tags=` OR filter, and the OpenAPI parameter it is published through |
| `tests/validation.rs` | Scalar type validation: wrong-typed fields rejected on create and update with the field named, valid and omitted fields accepted, and documents persisted before the change still readable |
| `tests/security_tests.rs` | Path traversal, symlink escape, malformed JSON, repository-level leniency, and concurrent writers |

Shared request builders and assertions live in `tests/common/mod.rs`. Cargo compiles only top-level files in `tests/` as test binaries, so a subdirectory module is shared across suites without running as one itself.

Run everything, a single layer, or one suite:

```sh
cargo test --all-targets --all-features   # unit + integration
cargo test --lib                          # unit tests only
cargo test --test attachments             # a single API suite
```

The crate exposes a library target (`src/lib.rs`) alongside the binary so integration tests can import `tucano_test::api` and drive the router directly, without binding a network port.

GitHub Actions runs workflow linting, formatting, Clippy, unit and integration tests, a release build, dependency auditing, secret scanning, and production container scanning. Main-branch builds and SemVer tags publish numbered container artifacts to GHCR.

The `checks`, `audit` and `sbom` jobs run inside a `rust` container, whose filesystem is discarded after every run. They mount a persistent Docker volume for `/usr/local/cargo/registry` and `/usr/local/cargo/git`, so the crates.io downloads a run does not already hold are fetched once and then reused, instead of every run depending on `static.crates.io` resolving. Only those two subdirectories are cached: the image's own `cargo` and `rustc` binaries stay in place, so a newer image is never shadowed by a stale cache.

Repository contribution and agent workflow rules are documented in [AGENTS.md](AGENTS.md).

## Documentation

| Document | Purpose |
| --- | --- |
| [docs/architecture/rust-service-core.md](docs/architecture/rust-service-core.md) | Why Rust, the layered service design, and delivery status |
| [docs/contracts/api-compatibility.md](docs/contracts/api-compatibility.md) | File-format and endpoint compatibility rules against the legacy implementation |
| [docs/contracts/test-case-versioning-plan.md](docs/contracts/test-case-versioning-plan.md) | Field names, snapshot shape, trigger rules, and addressing for test-case versioning and revision history |
| [docs/security/threat-model.md](docs/security/threat-model.md) | Trust boundaries, abuse cases, and security invariants |
| [AgentRules/](AgentRules/) | Organisation-wide engineering and process rules |
