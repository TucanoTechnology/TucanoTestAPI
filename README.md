# Tucano Test API

The Tucano Test API is the file-based test case management service for TucanoTCM: a Rust service
built on Axum that stores projects, suites, cases, runs, milestones, and configurations as JSON
documents on disk — no database — and exposes every operation through the documented HTTP contract
in [openapi.json](openapi.json).

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
│       │   ├── revisions/v<n>.json      snapshots written by a qualifying update
│       │   ├── steps/<n>/               attachments of one structured step
│       │   └── <attachments>
│       └── <test suite>/
│           ├── suite.json               suite details
│           └── <test case>/
│               ├── test-case.json
│               ├── revisions/v<n>.json
│               ├── steps/<n>/
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
   folder; run results are recorded inside the run's own document under `test_runs/` — a run is one
   flat `<id>.json` file, not a folder.

Milestones and test runs must never go silently stale when the source cases or suites they refer to
change: they either carry their own snapshot at inclusion time or record the history of the runs
they were included in with that run's results. A run records, per case, the case revision it
captured, so reading a finished run back never shows a version its source case no longer has.

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

## HTTP API

`openapi.json` is the authoritative contract: it is served at `/openapi.json` and rendered by the
Swagger UI at `/api-docs`. `tests/service.rs` checks it from both sides — that the document matches
the routes the router registers, and that the router serves every route the document names. The
surface is:

| Area | Routes |
| --- | --- |
| Health and contract | `GET /health`, `GET /openapi.json`, `GET /api-docs` |
| Projects | `GET`/`POST /projects`, `GET`/`PUT`/`DELETE /projects/{id}`, `POST /projects/{id}/duplicate`, parent-scoped suite and case creation (`/projects/{id}/test_suites`, `/projects/{id}/test_cases`) |
| Suites | `GET /test_suites`, `GET`/`PUT`/`DELETE /test_suites/{id}`, `POST /test_suites/{id}/duplicate`, parent-scoped case creation (`POST /test_suites/{id}/test_cases`) |
| Cases | `GET`/`PUT`/`DELETE /test_cases/{id}`, `POST /test_cases/{id}/duplicate`, attachments (`/test_cases/{id}/attachments`), step attachments (`/test_cases/{id}/steps/{step_index}/attachments`), revision history (`GET /test_cases/{id}/history`, `GET /test_cases/{id}/history/{version}`) |
| Runs | `GET`/`POST /test_runs`, `GET`/`PUT`/`DELETE /test_runs/{id}`, `POST /test_runs/{id}/duplicate`, suite and case inclusion (`/test_runs/{id}/test_suites`, `/test_runs/{id}/test_cases`), result recording (`POST /test_runs/{id}/results`), defect links (`/test_runs/{id}/results/{case_id}/defects`), imports (`POST /test_runs/{id}/import/junit`, `POST /test_runs/{id}/import/json`), configuration links (`/test_runs/{id}/configurations`) |
| Milestones | `GET`/`POST /milestones`, `GET`/`PUT`/`DELETE /milestones/{id}`, `POST /milestones/{id}/duplicate`, `GET /milestones/{id}/progress` |
| Configurations | `GET`/`POST /configurations`, `GET`/`PUT`/`DELETE /configurations/{id}` |
| Reports | `GET /reports/coverage` |

List endpoints share `?filter=`, `?tags=` (matched as an OR set), and — for runs, the only
collection with configuration references — `?configuration=`.

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

Build and run the Compose stack:

```sh
docker compose up -d --build
```

`docker-compose.yml` defines two services: `api`, built from this repository's `Dockerfile`, and
`gui`, built from a sibling `../Tucano-Test-GUI` checkout that must be present for the default
command. Start the API alone with `docker compose up -d --build api`.

| Service | Host port | Container port | Image |
| --- | --- | --- | --- |
| `api` | `3100` | `3000` | `tucano-test-api:local` |
| `gui` | `8080` | `8080` | `tucano-test-gui:local` |

The API runs as an unprivileged user (`uid 10001`) with a read-only root filesystem, a `/tmp` tmpfs,
and `no-new-privileges`; only `/data` and `/tmp` are writable. It stores inspectable JSON and
attachments in the data directory that Compose bind-mounts from the host `./data` folder at `/data`
(Docker creates the folder on first run), and the image also declares `/data` as a `VOLUME`. The
storage location is configurable through `TUCANO_DATA_DIR`. Keep test data and secrets out of version
control — the `.gitignore` excludes `/data/*`, so test data is never committed. A deployment that
prefers a managed named volume can mount `tucano-test-data:/data` instead; the container contract is
the path `/data`, never the volume name, as described in the deployment guide.

Interactive Swagger UI is available at `http://localhost:3100/api-docs`; the raw OpenAPI document is
at `http://localhost:3100/openapi.json`. A direct `docker run` of the image listens on `3000` unless
you map it elsewhere.

The service is unauthenticated today — it must not be exposed beyond a trusted network. The authentication
decision (deferred implementation, project-scoped RBAC, short-lived JWT plus refresh token, no database) is
recorded in [docs/security/authentication-decision.md](docs/security/authentication-decision.md) and tracked in
[#130](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130).

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

To validate a candidate build before it serves traffic, and to roll back to a previous build safely, follow [docs/deployment/canary-validation-and-rollback.md](docs/deployment/canary-validation-and-rollback.md).

The full deployment model — the JSON volume mount that is the only state, the Compose configuration and container hardening, single-node versus shared-storage scaling, and rollback to an immutable release tag — is in [docs/deployment/deployment-guide.md](docs/deployment/deployment-guide.md).

## Smoke validation

`scripts/smoke.sh` exercises a running API with a scratch CRUD round trip — health, create a project and a case, read both back, delete both, and confirm each deletion is observable — and exits non-zero on the first deviation. It needs `curl` and `python3`, removes its scratch data on exit, and accepts a base URL:

```sh
scripts/smoke.sh                       # defaults to http://localhost:3000
scripts/smoke.sh http://localhost:3100 # the Compose api service
scripts/smoke.sh http://localhost:3101 # any replica, for example a canary
```

Use it to validate a candidate build before promotion; the surrounding procedure is in [docs/deployment/canary-validation-and-rollback.md](docs/deployment/canary-validation-and-rollback.md).

## Test Data Cleanup

Helper scripts are provided in `scripts/` to wipe sample test data against a running API instance:

### Prerequisites

Node.js 18+ (uses native `fetch` and ES modules).

### Clearing Data

Wipes all milestones, test runs, test suites, projects, and test cases (with their attachments) from
the API. With no argument the script probes `http://localhost:3100`, then `http://localhost:8080/api`,
then `http://localhost:3000`, and uses the first base URL that answers `/health`, so a Compose stack
is found without arguments. Configurations are not removed.

```sh
# From repository root:
node scripts/clear-data.mjs
# or (if executable permissions are set):
./scripts/clear-data.mjs

# From within the scripts/ folder:
node clear-data.mjs

# Provide a custom API base URL if needed:
node scripts/clear-data.mjs http://localhost:3100
```

## Release numbering

Application releases use Semantic Versioning. Update the Cargo package version and create a protected `vMAJOR.MINOR.PATCH` tag for a release; release tags are immutable and must never be reused. Every push to `main` also publishes an immutable GHCR image tagged `build-<GitHub run number>`. Tagged releases publish both the SemVer tag and their build number, while the commit SHA remains the audit identity. Pull requests build and test without publishing release artifacts.

## Module layout

The API is layered so that each concern has exactly one home; a layer only depends on the layers
beneath it, and nothing below the HTTP layer knows about Axum:

| Path | Role |
| --- | --- |
| `src/models.rs` | The stored documents (projects, suites, cases, runs, milestones, configurations) in their legacy JSON shapes |
| `src/storage/` | The only code that touches the filesystem: the `Repository` trait, its `FileRepository` implementation (`fs.rs`), and the path layout and confinement rules (`layout.rs`) |
| `src/domain/` | The business rules behind `TestService<R: Repository>`: validation, identifier derivation and required fields, composition, duplication, milestone progress, result import, defect links, coverage aggregation, and error translation |
| `src/api/` | The HTTP layer: one module per resource (`projects`, `suites`, `cases`, `runs`, `milestones`, `configurations`), plus `reports.rs` for the coverage endpoint, `crud.rs` (the shared handler macros) and `error.rs` (the error envelope) |
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
| `tests/runs.rs` | Test run CRUD, validation, conflicts, missing resources, the case-version capture each run records, JUnit XML and JSON result import, and listing, linking and unlinking the defect links a result carries |
| `tests/cases.rs` | Test case CRUD, required fields, parent-scoped creation, copy/move composition, conflicts, missing resources, and versioning — the `version`/`lastModified` stamp, the `revisions/` snapshots a qualifying update writes, and the history endpoints that list and read them back |
| `tests/milestones.rs` | Milestone CRUD, validation, conflicts, duplication, and progress derived from the referenced runs |
| `tests/configurations.rs` | Configuration CRUD, validation, conflicts, missing resources, restart persistence, and use by a run |
| `tests/reports.rs` | The reports: the coverage report (per-suite and total case counts, the project scope filter, the global scope) and the run summary (the status buckets, the pass rate, the summed durations, the intersecting project/milestone/configuration and date filters), with the error answers for an unknown and an unusable identifier |
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

GitHub Actions runs workflow linting, formatting, Clippy, unit and integration tests, a release build, dependency auditing, secret scanning, production container scanning, and CycloneDX SBOM generation. Main-branch builds and SemVer tags publish numbered container artifacts to GHCR.

The `checks`, `audit` and `sbom` jobs run inside a `rust` container, whose filesystem is discarded after every run. They mount a persistent Docker volume for `/usr/local/cargo/registry` and `/usr/local/cargo/git`, so the crates.io downloads a run does not already hold are fetched once and then reused, instead of every run depending on `static.crates.io` resolving. Only those two subdirectories are cached: the image's own `cargo` and `rustc` binaries stay in place, so a newer image is never shadowed by a stale cache.

Repository contribution and agent workflow rules are documented in [AGENTS.md](AGENTS.md).

## Documentation

| Document | Purpose |
| --- | --- |
| [docs/architecture/rust-service-core.md](docs/architecture/rust-service-core.md) | Why Rust, the layered service design, and delivery status |
| [docs/architecture/gui-client-boundary.md](docs/architecture/gui-client-boundary.md) | The GUI client boundary and the generated-client strategy driven by `openapi.json` |
| [docs/contracts/api-compatibility.md](docs/contracts/api-compatibility.md) | File-format and endpoint compatibility rules against the legacy implementation |
| [docs/contracts/test-case-versioning-plan.md](docs/contracts/test-case-versioning-plan.md) | Field names, snapshot shape, trigger rules, and addressing for test-case versioning and revision history |
| [docs/contracts/file-format-versioning-plan.md](docs/contracts/file-format-versioning-plan.md) | The `formatVersion` storage marker: field, reader and writer rules, migration rules, and the rollback drill matrix |
| [docs/deployment/deployment-guide.md](docs/deployment/deployment-guide.md) | The deployment model: the JSON volume mount, Compose configuration, container hardening, scaling, and rollback to an immutable release tag |
| [docs/deployment/canary-validation-and-rollback.md](docs/deployment/canary-validation-and-rollback.md) | Canary validation, the scratch-CRUD smoke check, and safe rollback |
| [docs/security/threat-model.md](docs/security/threat-model.md) | Trust boundaries, abuse cases, and security invariants |
| [docs/security/authentication-decision.md](docs/security/authentication-decision.md) | The authentication decision (deferred) and its tracking ticket |
| [docs/security/scanning-policy.md](docs/security/scanning-policy.md) | The dependency-audit, secret-scan, container-scan, and SBOM policy CI enforces |
| [docs/roadmap.md](docs/roadmap.md) | The roadmap ordered by delivery priority, with the tracking issue for each item |
| [AgentRules/](AgentRules/) | Organisation-wide engineering and process rules |
