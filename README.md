# Tucano Test API

[![Workflows](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/lint.yml/badge.svg?branch=main&event=push&job=workflows)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/lint.yml)


[![Docs](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/docs.yml/badge.svg?branch=main&event=push&job=validate)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/docs.yml)

[![Rustdoc](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/build-test.yml/badge.svg?branch=main&event=push&job=rustdoc)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/build-test.yml)

[![Audit](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/security.yml/badge.svg?branch=main&event=push&job=audit)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/security.yml)

[![Release](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/release.yml/badge.svg?branch=main)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/release.yml)

The Tucano Test API is the file-based test case management service for TucanoTCM: a Rust service
built on Axum that stores projects, suites, cases, runs, milestones, and configurations as JSON
documents on disk — no database — and exposes every operation through the documented HTTP contract
in [openapi.json](openapi.json).

> **Not a developer?** User documentation lives in the [wiki](docs/wiki/README.md) and is published
> to the repository's GitHub Wiki on every merge to `main`. AI agents should read
> [AGENTS.md](AGENTS.md) before making changes.

## Prerequisites

| Requirement | Version | Purpose |
| --- | --- | --- |
| Rust toolchain | `1.98.0` (pinned by `rust-toolchain.toml`) | Build and test the API |
| `rustfmt` and `clippy` components | Bundled with the pinned toolchain | Formatting and lint gates |
| Docker Engine | 24 or newer | Build and run the production container |
| Docker Compose plugin | v2 | Local deployment via `docker compose` |
| Node.js | 18 or newer (native `fetch` and ES modules) | Run the seed, teardown and validation scripts |
| `actionlint` (optional) | `1.7.12` | Validate workflow files locally |

Install the toolchain with [rustup](https://rustup.rs); `rust-toolchain.toml` pins the exact version
automatically:

```sh
rustup show
cargo build --release
```

If you prefer not to install Rust locally, every command below also runs inside the pinned image:

```sh
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.98.0-bookworm cargo test --all-targets --all-features
```

## Editor setup

Recommended VS Code extensions are listed in `.vscode/extensions.json` and VS Code will offer to
install them when the workspace is opened:

- `rust-lang.rust-analyzer` — Rust language support
- `github.vscode-github-actions` — workflow authoring and validation
- `redhat.vscode-yaml` — YAML schema validation
- `tamasfe.even-better-toml` — `Cargo.toml` support

`.vscode/settings.json` maps `.github/workflows/*.yml` to the SchemaStore GitHub Actions schema so
workflow files validate correctly.

The GitHub Actions extension may report `Context access might be invalid: GITHUB_TOKEN` on
`.github/workflows/release.yml`. This is a known false positive: `GITHUB_TOKEN` is injected
automatically by GitHub Actions and is not a user-defined repository secret, so the extension cannot
resolve it while signed out. The workflows are validated in CI with `actionlint`, which reports no
issues.

## Running the application

The stack authenticates by default, so it needs a signing secret and a bootstrap account before it
starts. Copy the committed template to `.env` and set both values; `docker compose` loads `.env`
automatically from this directory, and `.env` is gitignored so the secret stays on the machine:

```sh
cp .env.example .env
# set TUCANO_JWT_SECRET (at least 32 bytes) and TUCANO_BOOTSTRAP_PASSWORD, then:
docker compose up -d --build
```

Generate a signing secret with, for example,
`node -e 'process.stdout.write(require("node:crypto").randomBytes(32).toString("base64url"))'`.
Compose refuses to start while either required value is unset, rather than falling back to an
anonymous stack.

`docker-compose.yml` defines two services: `api`, built from this repository's `Dockerfile`, and
`gui`, built from a sibling `../Tucano-Test-GUI` checkout that must be present for the default
command. Start the API alone with `docker compose up -d --build api`.

| Service | Host port | Container port | Image |
| --- | --- | --- | --- |
| `api` | `3100` | `3000` | `tucano-test-api:local` |
| `gui` | `8080` | `8080` | `tucano-test-gui:local` |

Those `…:local` names are local build outputs, not release tags — every `docker compose up --build`
retags them, so the image that ran before loses the name. Before rebuilding, record the running
container's image id and pin it under a rollback-only tag; a Compose rollback recreates the service
from that id with `--no-build`. The exact sequence is in the
[runbook's Rollback section](docs/deployment/canary-validation-and-rollback.md#rollback).

The `api` service sets `TUCANO_AUTH_REQUIRED=true`, so every guarded route needs a bearer token from
`POST /auth/login` — sign in with the bootstrap account from `.env`. That is the safe posture the
shipped file is required to produce ([audit finding F-178-1](docs/security/audit-s3-container-and-deployment.md)).

To run anonymously on a single-user machine, set `TUCANO_AUTH_REQUIRED=false` in `.env`. Do not do
that on a machine anything else can reach; the API is published on every interface, so a deployment
that needs to stay reachable should either keep authentication on or restrict the publish to
loopback by changing the port mapping to `127.0.0.1:3100:3000`.

Interactive Swagger UI is available at `http://localhost:3100/api-docs`; the raw OpenAPI document is
at `http://localhost:3100/openapi.json`; the request counters are at
`http://localhost:3100/metrics`.

The API runs as an unprivileged user (`uid 10001`) with a read-only root filesystem, a `/tmp` tmpfs,
all Linux capabilities dropped, and `no-new-privileges`; only `/data` and `/tmp` are writable. It
stores inspectable JSON and attachments in the data directory that Compose bind-mounts from the host
`./data` folder at `/data` (Docker creates the folder on first run). Keep test data and secrets out
of version control — the `.gitignore` excludes `/data/*`, so test data is never committed.

The full deployment model — container hardening, the optional configuration file, scaling, and
rollback — is in the [deployment guide](docs/deployment/deployment-guide.md).

## Logging and metrics

Every request opens one `http.request` span carrying the method, the path, the query string (every
secret query parameter redacted), the status and the latency. Every mutating operation writes one
`tucano.audit` event naming the action, the resource, the identifier and the outcome, alongside the
error code when it was refused. No request body and no attachment's contents are ever logged.

`TUCANO_LOG` is the `tracing-subscriber` directive set, defaulting to `info` — the request spans, the
audit lines and the failures, without the per-connection noise `debug` adds. `TUCANO_LOG_FORMAT` is
`compact` (the default, one human-readable line per event, coloured only when stdout is a terminal)
or `json` (one object per event, uncoloured, for a collector to parse). Both are read once at
startup, so an unparseable directive set or an unknown format stops the server rather than a
request.

`GET /metrics` serves Prometheus counters in the text exposition format the API renders itself — no
new endpoint dependency and no token, since the route sits with the other unguarded ones. Each
series counts requests by method, by the first segment of the matched route template, and by status
class.

## Module layout

The API is layered so that each concern has exactly one home; a layer only depends on the layers
beneath it, and nothing below the HTTP layer knows about Axum:

| Path | Role |
| --- | --- |
| `src/models.rs` | The stored documents (projects, suites, cases, runs, milestones, configurations) in their legacy JSON shapes |
| `src/storage/` | The only code that touches the filesystem: the `Repository` trait, its `FileRepository` implementation (`fs.rs`), and the path layout and confinement rules (`layout.rs`) |
| `src/domain/` | The business rules behind `TestService<R: Repository>`: validation, identifier derivation and required fields, composition, duplication, milestone progress, result import, defect links, coverage aggregation, and error translation |
| `src/api/` | The HTTP layer: one module per resource (`projects`, `suites`, `cases`, `runs`, `milestones`, `configurations`), plus `reports.rs` for the coverage endpoint, `crud.rs` (the shared handler macros) and `error.rs` (the error envelope) |
| `src/auth/` | Authentication: `config.rs` (the settings contract and the environment-over-file precedence), `password.rs`, `token.rs`, `store.rs` (accounts, refresh tokens and grants below `auth/`), `session.rs` (the sign-in/refresh/sign-out rules), `bootstrap.rs` (the first account), and `seed.rs` (the demo accounts the seed dataset needs) |
| `src/config.rs` | The optional startup configuration file: its versioned strict schema, the loader, and the environment-over-file precedence rule |
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
scripts/coverage-check.sh
```

`scripts/coverage-check.sh` runs the full suite and then checks the contract: every operation in
`openapi.json` must have been driven to a successful answer by the tests that ran. It replaces
`cargo test --all-targets --all-features`, which it runs as its first pass — see [Testing](#testing).

Validate the workflow files with the same linter CI uses:

```sh
docker run --rm -v "$PWD:/repo:ro" --workdir /repo rhysd/actionlint:1.7.12 -color
```

All CI/CD jobs must pass locally before committing and raising a PR. See [AGENTS.md](AGENTS.md) for
the full contribution and agent workflow rules.

## Testing

Tests are split into two layers and both run in CI on every push and pull request:

- **Unit tests** live beside the code in `src/models.rs`, `src/storage/`, and `src/domain/`. They
  cover legacy JSON compatibility, payload validation, composition and duplication rules, milestone
  progress, atomic writes, path confinement, attachment storage, concurrent writers, and file
  permissions.
- **Integration tests** live in `tests/` and exercise the HTTP surface in-process through the Axum
  router against a temporary data directory. Each API area has its own suite:

| Suite | Covers |
| --- | --- |
| `tests/service.rs` | Health, readiness and storage diagnostics, OpenAPI document and its error contract, Swagger UI, malformed bodies, traversal rejection, the identifier and size-limit error answers, the on-disk tree layout, persistence across restarts, and the coverage table that fails when a documented operation has no covering test |
| `tests/projects.rs` | Project CRUD, validation, conflicts, error envelopes |
| `tests/suites.rs` | Test suite CRUD, parent-scoped creation, copy/move composition, ambiguity conflicts, missing resources |
| `tests/runs.rs` | Test run CRUD, validation, conflicts, missing resources, the case-version capture each run records, recording, replacing and removing a result, JUnit XML and JSON result import, and listing, linking and unlinking the defect links a result carries |
| `tests/cases.rs` | Test case CRUD, required fields, parent-scoped creation, copy/move composition, conflicts, missing resources, and versioning — the `version`/`lastModified` stamp, the `revisions/` snapshots a qualifying update writes, and the history endpoints |
| `tests/milestones.rs` | Milestone CRUD, validation, conflicts, duplication, and progress derived from the referenced runs |
| `tests/configurations.rs` | Configuration CRUD, validation, conflicts, missing resources, restart persistence, and use by a run |
| `tests/metadata.rs` | The context-bar listings: the distinct, sorted release and environment names derived from the milestones and configurations in the projects the caller reaches, filtered rather than refused, the empty array an installation with neither answers, and the document that publishes both |
| `tests/reports.rs` | The reports: the coverage report (per-suite and total case counts, the project scope filter, the global scope) and the run summary (the status buckets, the pass rate, the summed durations, the intersecting project/milestone/configuration and date filters), with the error answers for an unknown and an unusable identifier |
| `tests/request_id.rs` | The request id: the minted `X-Request-Id` on a request that sends none, the verbatim echo of an inbound one, the replacement of an empty header, distinct ids per request, the `requestId` the error envelope carries, the header a plain-text rejection still carries, and the id the request span is given |
| `tests/observability.rs` | The request span every request opens (method, path, status, latency), the `/metrics` counters in the Prometheus text format, the audit line a mutation writes and the failure line a refused one writes, and the credentials, bodies and attachment contents the whole-process log capture never holds |
| `tests/attachments.rs` | Upload, download, delete, content types, removal with the parent test case |
| `tests/tags.rs` | The `tags` array on projects, suites, cases and runs, the shared `?tags=` OR filter, and the OpenAPI parameter it is published through |
| `tests/validation.rs` | Scalar type validation: wrong-typed fields rejected on create and update with the field named, valid and omitted fields accepted, and documents persisted before the change still readable |
| `tests/security_tests.rs` | Path traversal, symlink escape, malformed JSON, repository-level leniency, and concurrent writers |
| `tests/route_coverage.rs` | What the router actually served: every operation in `openapi.json` must have been driven to a successful answer during the run. Reads the recording the shared harness writes and asserts nothing unless `scripts/coverage-check.sh` turns it on |

Shared request builders and assertions live in `tests/common/mod.rs`. Cargo compiles only top-level
files in `tests/` as test binaries, so a subdirectory module is shared across suites without running
as one itself.

`tests/service.rs` also carries a coverage table: one row per operation in `openapi.json`, each
naming the test that drives it and asserts its success. The `every_documented_operation_has_a_covering_test`
guard compares that table against the served document in both directions, so an operation added to the
schema without a covering test fails the build, as does a row left behind by a renamed or removed one.
A row may not name one of the shared role or malformation sweeps, which drive many routes but assert
only the status they must refuse with. Because the API's contract is what `openapi.json` declares,
this is the coverage the project enforces; line coverage is not measured.

The table on its own is a declaration — it proves that the test it names exists, not that the test
reaches the route. `scripts/coverage-check.sh` adds the observed half: the shared harness records the
registered template of every request the router serves, and `tests/route_coverage.rs` then requires a
successful answer for each operation in the document. An operation no test drove, or one that only
ever answered an error, fails the check by name. It attributes a success to the run as a whole rather
than to the row's own test — the recording carries no test identity — so the two halves are read
together. Run it instead of a bare `cargo test` when you want the full guarantee:

```sh
scripts/coverage-check.sh                  # record, then check (fills target/route-coverage/hits.tsv)
cargo test --all-targets --all-features    # the same suite run, recording only
```

A plain `cargo test` stays green while recording: the check reads the recording the suites write, and
libtest orders the test binaries arbitrarily, so recording and checking cannot be one pass. The script
truncates its recording first, so a stale file from an earlier run can never stand in for a run that
covered less.

Run everything, a single layer, or one suite:

```sh
cargo test --all-targets --all-features   # unit + integration
cargo test --lib                          # unit tests only
cargo test --test attachments             # a single API suite
```

The crate exposes a library target (`src/lib.rs`) alongside the binary so integration tests can
import `tucano_test::api` and drive the router directly, without binding a network port.

GitHub Actions runs workflow linting, formatting, Clippy, unit and integration tests, a release build,
dependency auditing, secret scanning, production container scanning, and CycloneDX SBOM generation.
Main-branch builds and SemVer tags publish numbered container artifacts to GHCR.

The `checks`, `audit` and `sbom` jobs run inside a `rust` container, whose filesystem is discarded
after every run. They mount a persistent Docker volume for `/usr/local/cargo/registry` and
`/usr/local/cargo/git`, so the crates.io downloads a run does not already hold are fetched once and
then reused, instead of every run depending on `static.crates.io` resolving. Only those two
subdirectories are cached: the image's own `cargo` and `rustc` binaries stay in place, so a newer
image is never shadowed by a stale cache.

## Test-data generator

`scripts/seed.mjs` builds a complete demo environment against a running deployment over HTTP, and
`scripts/teardown.mjs` removes exactly what the seed created. The dataset is generated, never stored
— it cannot drift from the API.

```sh
# Start a deployment with auth enforced, then seed:
node scripts/seed.mjs http://localhost:3100

# Remove exactly what the seed created:
node scripts/teardown.mjs http://localhost:3100
```

The full usage guide — the three audiences, environment variables, the auth exception, extending the
generator when a feature lands, and the validation step — is in the [test-data generator
guide](docs/testing/test-data-generator-guide.md). The dataset contract is the [seed dataset
specification](docs/testing/seed-dataset-spec.md).

### Scripts in `scripts/`

| Path | Role |
| --- | --- |
| `demo.sh` | Brings up a Compose stack, seeds it, runs `smoke.sh`, then validates the dataset with `validate-seed.mjs` |
| `smoke.sh` | Scratch CRUD round trip against a running API, for validating a candidate build |
| `seed.mjs` | Builds the demo dataset of `docs/testing/seed-dataset-spec.md` over HTTP |
| `validate-seed.mjs` | Asserts the seeded dataset through the API, including the refusals a non-admin receives |
| `teardown.mjs` | Removes exactly what `seed.mjs` created, by identifier, and reports anything it leaves in place |
| `check-matrix.mjs` | Fails when a route in `openapi.json` and a row of the dataset's coverage matrix disagree |
| `generate-operations-reference.mjs` | Renders `docs/generated/operations-reference.md` from `openapi.json`; `--check` fails on any drift |
| `check-docs-links.mjs` | Fails when a documentation link breaks, a page is missing from `SUMMARY.md` or the README table, or the wiki index is incomplete |
| `sync-github-wiki.mjs` | Stages `docs/wiki/` as GitHub Wiki pages (`--out <dir>`); `--check` verifies the flat-namespace mapping |
| `clear-data.mjs` | Unscoped wipe: empties every sample collection a run or seed left behind |
| `fixtures/` | The small files the seed uploads: a case attachment, a step attachment, and the JUnit report it imports |

## Release numbering

Application releases use Semantic Versioning. Update the Cargo package version and create a
protected `vMAJOR.MINOR.PATCH` tag for a release; release tags are immutable and must never be
reused. Every push to `main` also publishes an immutable GHCR image tagged `build-<GitHub run
number>`. Tagged releases publish both the SemVer tag and their build number, while the commit SHA
remains the audit identity. Pull requests build and test without publishing release artifacts.

## Storage concept

Tucano Test stores all state as folders and JSON files on the filesystem — there is no database.
The folder layout mirrors the conceptual domain hierarchy: **project → suite → test case**, with
runs, milestones and configurations as flat `<id>.json` files inside their project.

The full domain model, folder layout, and composition semantics are in the [storage concept and API
reference](docs/reference/storage-and-api.md). The storage layout decision of record is
[ADR: storage layout v3](docs/architecture/adr-storage-layout-v3.md).

## HTTP API

`openapi.json` is the authoritative contract, served at `/openapi.json` and rendered by Swagger UI
at `/api-docs`. The full route table, list filters, and compatibility routes are in the [storage
concept and API reference](docs/reference/storage-and-api.md).

## Authentication

The shipped `docker-compose.yml` turns authentication on (`TUCANO_AUTH_REQUIRED=true`) and takes the
signing secret and the bootstrap account from `.env`. The service's own default remains `false`, so
a deployment that supplies its own container definition and does not opt in still runs anonymously —
the Compose file is the safe default, not the service.

With authentication on, guarded routes require a bearer token authorized against
**project-scoped RBAC**: a role (`viewer`, `editor`, `owner`) granted per project, plus a
`systemAdmin` account that reaches everything. The full role model, environment variables, the
optional configuration file, and the error contract are in the [storage concept and API
reference](docs/reference/storage-and-api.md). The decision of record is the [authentication
decision](docs/security/authentication-decision.md); the configuration reference lists every
[setting](docs/deployment/configuration-reference.md).

## Errors and request limits

Every application rejection answers `{"error": {"code": "…", "message": "…"}}`. Published codes, the
body-size limit, and the multipart framing behaviour are in the [storage concept and API
reference](docs/reference/storage-and-api.md). The error contract reconciliation is in
[API compatibility](docs/contracts/api-compatibility.md).

## Documentation

| Document | Purpose |
| --- | --- |
| [docs/reference/storage-and-api.md](docs/reference/storage-and-api.md) | The storage concept, HTTP API routes, authentication, configuration, errors, and scaling |
| [docs/architecture/rust-service-core.md](docs/architecture/rust-service-core.md) | Why Rust, the layered service design, and delivery status |
| [docs/architecture/gui-client-boundary.md](docs/architecture/gui-client-boundary.md) | The GUI client boundary and the generated-client strategy driven by `openapi.json` |
| [docs/architecture/adr-object-storage.md](docs/architecture/adr-object-storage.md) | ADR: why object storage (S3) is declined as a persistence backend and the file-based invariant is upheld (#181) |
| [docs/architecture/storage-backends.md](docs/architecture/storage-backends.md) | The storage backends and their operational implications (#186): the one implemented backend, what it guarantees, backup/scaling/rollback consequences, and the declined object-store backend versus the permitted external mirror |
| [docs/architecture/adr-storage-layout-v3.md](docs/architecture/adr-storage-layout-v3.md) | ADR: storage layout v3 (#215) — runs, milestones and configurations live inside their project and are governed by it |
| [docs/architecture/wiki-structure-and-publication.md](docs/architecture/wiki-structure-and-publication.md) | The wiki decision: source of truth, publication mechanism, page inventory, and the drift-prevention rule |
| [docs/architecture/performance.md](docs/architecture/performance.md) | Criterion benchmark baselines for CRUD, listing, validation, and upload workloads (#97) |
| [docs/contracts/api-compatibility.md](docs/contracts/api-compatibility.md) | File-format and endpoint compatibility rules against the legacy implementation |
| [docs/contracts/test-case-versioning-plan.md](docs/contracts/test-case-versioning-plan.md) | Field names, snapshot shape, trigger rules, and addressing for test-case versioning and revision history |
| [docs/contracts/file-format-versioning-plan.md](docs/contracts/file-format-versioning-plan.md) | The `formatVersion` storage marker: field, reader and writer rules, migration rules, and the rollback drill matrix |
| [docs/deployment/deployment-guide.md](docs/deployment/deployment-guide.md) | The deployment model: the JSON volume mount, Compose configuration, the optional configuration file, container hardening, scaling, and rollback to an immutable release tag — or, for the Compose local build, to the running container's pinned image id |
| [docs/deployment/config.example.json](docs/deployment/config.example.json) | The configuration-file template, kept valid against the loader's schema by a unit test |
| [docs/deployment/configuration-reference.md](docs/deployment/configuration-reference.md) | Every setting: environment variable, file key, default, sensitivity, precedence rules, validation rules, and the encrypted-secret envelope format (#190) |
| [docs/deployment/canary-validation-and-rollback.md](docs/deployment/canary-validation-and-rollback.md) | Canary validation, the scratch-CRUD smoke check, and safe rollback |
| [docs/security/threat-model.md](docs/security/threat-model.md) | Trust boundaries, abuse cases, and security invariants |
| [docs/security/authentication-decision.md](docs/security/authentication-decision.md) | The authentication decision and its implemented tracking ticket (#130) |
| [docs/security/configuration-decision.md](docs/security/configuration-decision.md) | The configuration and secrets decision (#187): environment variables versus a unified file, precedence, and key management |
| [docs/security/scanning-policy.md](docs/security/scanning-policy.md) | The dependency-audit, secret-scan, container-scan, and SBOM policy CI enforces |
| [docs/security/audit-scope.md](docs/security/audit-scope.md) | The security audit's scope, methodology, finding template, and severity rubric (#175) |
| [docs/security/audit-design-176-178.md](docs/security/audit-design-176-178.md) | The design brief shared by the S1–S3 audit reports: what each report covers and the method they follow |
| [docs/security/audit-s4-dependencies-and-supply-chain.md](docs/security/audit-s4-dependencies-and-supply-chain.md) | The S4 supply-chain audit report — dependency advisories, CI controls, action and image pinning, the SBOM, and the workflow permissions model (#179) |
| [docs/security/audit-s3-container-and-deployment.md](docs/security/audit-s3-container-and-deployment.md) | The S3 container and deployment audit report — image and Compose hardening, secret delivery, the volume as the only state, network exposure, rollback, and the CI security jobs measured against the scanning policy (#178) |
| [docs/security/audit-s2-storage-and-filesystem.md](docs/security/audit-s2-storage-and-filesystem.md) | The S2 storage and filesystem audit report — the layout contract, identifier validation, permissions, atomicity, the lock, attachment and revision publication, the overwrite contract, the error surface, and the configuration-file boundary (#177) |
| [docs/security/audit-s1-http-surface.md](docs/security/audit-s1-http-surface.md) | The S1 HTTP surface audit report — the served route contract and its classification, authn/authz and IDOR, identifier validation and traversal, upload abuse, the error envelope, and the session/token lifecycle (#176) |
| [docs/testing/seed-dataset-spec.md](docs/testing/seed-dataset-spec.md) | The demo/seed dataset: the feature coverage matrix, the target tree below `TUCANO_DATA_DIR`, the API calls that produce it, and the teardown scope |
| [docs/testing/test-data-generator-guide.md](docs/testing/test-data-generator-guide.md) | Using the test-data generator as a demo, a fixture and a test bed, and how to extend it when a feature lands (#196) |
| [docs/testing/fuzz-and-property-tests.md](docs/testing/fuzz-and-property-tests.md) | The property suite that runs in `cargo test`, the fuzz targets under `fuzz/`, and how to run them (#100) |
| [docs/wiki/README.md](docs/wiki/README.md) | **User wiki** — index of the user-facing guides: installation, feature how-to, API quickstart, and operations. Mirrored to the repository's GitHub Wiki on every merge to `main` |
| [docs/wiki/getting-started.md](docs/wiki/getting-started.md) | Install the Compose stack, verify it is up, turn authentication on, and create a first project, suite and case (#171) |
| [docs/wiki/projects-suites-and-cases.md](docs/wiki/projects-suites-and-cases.md) | The container hierarchy, parent-scoped creation, and how membership is stored as folders (#172) |
| [docs/wiki/steps-and-attachments.md](docs/wiki/steps-and-attachments.md) | Structured test steps, case attachments, and per-step attachments (#172) |
| [docs/wiki/composing-and-duplicating.md](docs/wiki/composing-and-duplicating.md) | `copy` versus `move` inclusion semantics and duplication (#172) |
| [docs/wiki/tags-and-configurations.md](docs/wiki/tags-and-configurations.md) | Tagging projects, suites, cases, and runs; the shared `?tags=` filter; environment configurations (#172) |
| [docs/wiki/test-runs-and-results.md](docs/wiki/test-runs-and-results.md) | Point-in-time runs, recording results, and defect links (#172) |
| [docs/wiki/imports-and-reports.md](docs/wiki/imports-and-reports.md) | JUnit XML and JSON import, plus the coverage and summary reports (#172) |
| [docs/wiki/milestones.md](docs/wiki/milestones.md) | Milestone progress derived from referenced runs (#172) |
| [docs/wiki/case-versioning-and-history.md](docs/wiki/case-versioning-and-history.md) | The `version` stamp, `revisions/` snapshots, and the history endpoints (#172) |
| [docs/wiki/api-and-authentication.md](docs/wiki/api-and-authentication.md) | Zero to an authenticated call, the error envelope, request limits, and why Swagger is the contract (#173) |
| [docs/wiki/operations-and-troubleshooting.md](docs/wiki/operations-and-troubleshooting.md) | Running the service: the data directory as the only state, health and readiness, scaling, backup and restore, rollback, and a troubleshooting FAQ (#174) |
| [docs/roadmap.md](docs/roadmap.md) | The roadmap ordered by delivery priority, with the tracking issue for each item |
| [docs/roadmap-v1-gap-analysis.md](docs/roadmap-v1-gap-analysis.md) | V1 release gate: gap taxonomy, P0/P1 re-verification, startable frontier, v1 blocker set and sub-task decomposition (#241) |
| [docs/SUMMARY.md](docs/SUMMARY.md) | The table of contents of the published documentation site — the entry point that lists every page |
| [docs/generated/operations-reference.md](docs/generated/operations-reference.md) | Generated from `openapi.json`: the index of every route, method, `operationId` and summary |
| [AgentRules/](AgentRules/) | Organisation-wide engineering and process rules |
