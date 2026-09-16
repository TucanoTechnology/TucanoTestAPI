# Tucano Test API

[![CI](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/ci.yml)
[![Security](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/security.yml/badge.svg?branch=main)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/security.yml)
[![Release](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/release.yml/badge.svg?branch=main)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/release.yml)

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
contains a JSON file with its details plus any supplementary files that belong to it. It is
described below, and a running deployment writes a working example of all of it that you can read
back: see [Test-data generator](#test-data-generator).

```text
TUCANO_DATA_DIR/
└── projects/
    └── <project>/
        ├── project.json                 project details
        ├── test_runs/<id>.json          point-in-time runs and their results
        ├── milestones/<id>.json         milestone details
        ├── configurations/<id>.json     environment configurations
        ├── <test case>/                 case data directly in the project
        │   ├── test-case.json           case details, steps, expected results
        │   ├── revisions/v<n>.json      snapshots written by a qualifying update
        │   ├── steps/<n>/               attachments of one structured step
        │   └── <attachments>
        └── <test suite>/
            ├── suite.json               suite details
            └── <test case>/
                ├── test-case.json
                ├── revisions/v<n>.json
                ├── steps/<n>/
                └── <attachments>
```

Runs, milestones and configurations are stored as a single flat `<id>.json` file inside their
project, in a reserved subfolder per resource — `test_runs/`, `milestones/` and `configurations/`
are not suite or case names. The project folder is their only home, and it is what governs them.

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
as `POST /projects/{id}/test_runs {"name": "nightly"}` stores a run that reads back as its typed
model instead of one that fails to load. The rules are recorded in
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
   folder; run results are recorded inside the run's own document under the project's `test_runs/`
   — a run is one flat `<id>.json` file, not a folder.

Milestones and test runs must never go silently stale when the source cases or suites they refer to
change: they either carry their own snapshot at inclusion time or record the history of the runs
they were included in with that run's results. A run records, per case, the case revision it
captured, so reading a finished run back never shows a version its source case no longer has.

Three API semantics follow from this concept and apply to every composition request:

- **A real parent is required at creation.** A test suite is created inside its project and a test
  case inside its project or a test suite; a test run, milestone and configuration are created
  inside the project that owns them too. Nothing is created in a standalone top-level pool. The
  on-disk tree mirrors these homes: a suite folder lives under its project, a case folder under its
  project or its suite, and a run, milestone or configuration is one flat `<id>.json` in the
  reserved subfolder of its project. The creation endpoints are parent-scoped —
  `POST /projects/{id}/test_suites`, `POST /projects/{id}/test_cases`,
  `POST /test_suites/{id}/test_cases`, `POST /projects/{id}/test_runs`,
  `POST /projects/{id}/milestones` and `POST /projects/{id}/configurations`; the retired flat
  `POST /test_suites`, `POST /test_cases`, `POST /test_runs`, `POST /milestones` and
  `POST /configurations` answer `400 Bad Request` naming their replacement. Reads remain global —
  listing and retrieval search the whole tree, so an entity is always findable regardless of home,
  and each project also publishes a parent-scoped list of the runs, milestones and configurations
  it holds.
- **Inclusion is copy by default and move opt-in.** Adding an existing case or suite to another
  parent accepts `"mode": "copy" | "move"` and defaults to `copy`: `copy` duplicates the entity
  under the target parent (duplicate-on-include) while the source keeps its home and both copies
  are editable independently; `move` relocates the entity so the target parent becomes its only
  home. Test runs always copy at inclusion — they snapshot the selected cases and suites and never
  own them.
- **Identifiers are unique where they live.** A project id is globally unique; a suite id is unique
  within its project, and a case id is unique within its parent. A run, milestone and configuration
  id is unique within the project that holds it. Copy-on-include may therefore place the same id
  under several parents; a document-level route (`GET`/`PUT`/`DELETE /test_cases/{id}`,
  attachments, duplicate) operates on the one occurrence when it is unique and answers
  `409 Conflict`, naming the parent-scoped routes, when it is ambiguous. The global document routes
  for runs, milestones and configurations resolve the same way. Listing routes never fail on
  duplicates; they de-duplicate.

This concept is enforced for agent work in [AGENTS.md](AGENTS.md); the storage layout it describes
is recorded in
[docs/architecture/adr-storage-layout-v3.md](docs/architecture/adr-storage-layout-v3.md).

## HTTP API

`openapi.json` is the authoritative contract: it is served at `/openapi.json` and rendered by the
Swagger UI at `/api-docs`. `tests/service.rs` checks it from both sides — that the document matches
the routes the router registers, and that the router serves every route the document names. The
surface is:

| Area | Routes |
| --- | --- |
| Health, readiness and contract | `GET /health`, `GET /ready`, `GET /diagnostics`, `GET /openapi.json`, `GET /api-docs` |
| Authentication | `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`, `GET /auth/me` |
| Projects | `GET`/`POST /projects`, `GET`/`PUT`/`DELETE /projects/{id}`, `POST /projects/{id}/duplicate`, and the project's own children: suite and case creation (`POST /projects/{id}/test_suites`, `POST /projects/{id}/test_cases`), run, milestone and configuration creation (`POST /projects/{id}/test_runs`, `POST /projects/{id}/milestones`, `POST /projects/{id}/configurations`) with their parent-scoped lists and deletes (`GET`/`DELETE` on `/projects/{id}/test_runs`, `/projects/{id}/milestones`, `/projects/{id}/configurations`, and each `/{child_id}`) |
| Suites | `GET /test_suites`, `GET`/`PUT`/`DELETE /test_suites/{id}`, `POST /test_suites/{id}/duplicate`, parent-scoped case creation (`POST /test_suites/{id}/test_cases`) |
| Cases | `GET`/`PUT`/`DELETE /test_cases/{id}`, `POST /test_cases/{id}/duplicate`, attachments (`/test_cases/{id}/attachments`), step attachments (`/test_cases/{id}/steps/{step_index}/attachments`), revision history (`GET /test_cases/{id}/history`, `GET /test_cases/{id}/history/{version}`) |
| Runs | `GET /test_runs`, parent-scoped creation, listing and deletion (`GET`/`POST /projects/{id}/test_runs`, `DELETE /projects/{id}/test_runs/{run_id}`), `GET`/`PUT`/`DELETE /test_runs/{id}`, `POST /test_runs/{id}/duplicate`, suite and case inclusion (`/test_runs/{id}/test_suites`, `/test_runs/{id}/test_cases`), result recording (`POST /test_runs/{id}/results`), defect links (`/test_runs/{id}/results/{case_id}/defects`), imports (`POST /test_runs/{id}/import/junit`, `POST /test_runs/{id}/import/json`), configuration links (`/test_runs/{id}/configurations`) |
| Milestones | `GET /milestones`, parent-scoped creation, listing and deletion (`GET`/`POST /projects/{id}/milestones`, `DELETE /projects/{id}/milestones/{milestone_id}`), `GET`/`PUT`/`DELETE /milestones/{id}`, `POST /milestones/{id}/duplicate`, `GET /milestones/{id}/progress` |
| Configurations | `GET /configurations`, parent-scoped creation, listing and deletion (`GET`/`POST /projects/{id}/configurations`, `DELETE /projects/{id}/configurations/{config_id}`), `GET`/`PUT`/`DELETE /configurations/{id}` |
| Reports | `GET /reports/coverage`, `GET /reports/summary` |

List endpoints share `?filter=`, `?tags=` (matched as an OR set), and — for runs, the only
collection with configuration references — `?configuration=`. The retired flat creation routes
(`POST /test_suites`, `POST /test_cases`, `POST /test_runs`, `POST /milestones`,
`POST /configurations`) are still registered, but only to answer `400 Bad Request` with the route
that replaced them. The bare global scans (`GET /test_suites`, `GET /test_cases`, `GET /test_runs`,
`GET /milestones`, `GET /configurations`) also stay served for compatibility and are deliberately
absent from `openapi.json`; the documented way to list a project's runs, milestones and
configurations is the parent-scoped route.

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

### Authentication

With `TUCANO_AUTH_REQUIRED` turned on, every operation but `GET /health`, `GET /ready`,
`GET /diagnostics`, `GET /openapi.json`, `GET /api-docs`, `POST /auth/login`, and
`POST /auth/refresh` requires a bearer access token and is
authorized against **project-scoped RBAC**: a role (`viewer`, `editor`, `owner`) granted per project,
plus a `systemAdmin` account that reaches everything. A caller reaches only the projects it was
granted; listings are filtered down to them rather than refused, and direct reads or writes of a
project it does not reach answer `403`. Creating a project and duplicating one require the system
administrator. A milestone and a configuration are project resources like everything else: creating
one needs `editor` in the project it is created in (`owner` for a milestone), reading one needs
`viewer` there, and a run may reference a suite, case or configuration from any project the caller
reaches. Writing a run needs the role in its home project and in every project its `projects` array
names.

| Variable | Default | Purpose |
| --- | --- | --- |
| `TUCANO_DATA_DIR` | `./data` | The only state: where projects, cases, runs and the `auth/` store live. Environment-only. |
| `PORT` | `3000` | The port the API binds. Environment-only. |
| `TUCANO_CONFIG_FILE` | — | Path to the optional configuration file described below. Environment-only; unset means no file. |
| `TUCANO_AUTH_REQUIRED` | `false` | Require and enforce a bearer token on every guarded route. When off, every guard returns and the API is anonymous. |
| `TUCANO_JWT_SECRET` | — | The HS256 signing secret. Required when auth is on; at least 32 bytes. |
| `TUCANO_JWT_SECRET_FILE` | — | A file to read the secret from. Set this **or** `TUCANO_JWT_SECRET`, never both. |
| `TUCANO_ACCESS_TOKEN_TTL` | `15m` | Access-token lifetime; accepts a duration such as `900s` or `15m`. |
| `TUCANO_REFRESH_TOKEN_TTL` | `14d` | Refresh-token lifetime. Refresh tokens are rotated on every use. |
| `TUCANO_BOOTSTRAP_USERNAME` | — | A `systemAdmin` account created at startup when the store holds no accounts. Set with `TUCANO_BOOTSTRAP_PASSWORD`. |
| `TUCANO_BOOTSTRAP_PASSWORD` | — | The password for the bootstrap account, stored only as an Argon2id hash. |

Accounts and grants live beside the data, under `TUCANO_DATA_DIR/auth/`; the deployment stays
database-free. The decision of record is
[docs/security/authentication-decision.md](docs/security/authentication-decision.md) and the
enforcement matrix lives in `tests/auth.rs`.

### Configuration file

Every setting above can also be supplied by an optional JSON file named by `TUCANO_CONFIG_FILE`.
The file is read **once at startup**, before the listener binds, into an immutable value: a file the
server cannot read, cannot parse, or does not recognise is a startup failure, never a request
failure and never a silent fallback. The model and its rationale are in
[docs/security/configuration-decision.md](docs/security/configuration-decision.md); the template is
[docs/deployment/config.example.json](docs/deployment/config.example.json):

```json
{
  "version": 1,
  "auth_required": true,
  "jwt_secret": null,
  "jwt_secret_file": "/run/secrets/jwt",
  "access_token_ttl": "15m",
  "refresh_token_ttl": "14d",
  "bootstrap_username": null,
  "bootstrap_password": null
}
```

```sh
TUCANO_CONFIG_FILE=/etc/tucano-test/config.json \
  ./tucano-test
```

- **Precedence is resolved per key, not per source: environment, then file, then the built-in
  default.** A file that sets `access_token_ttl` while the environment sets `TUCANO_JWT_SECRET`
  applies both; the environment wins only where the two name the same setting.
- **`version` is mandatory and must be `1`.** A file declaring another version is refused by number
  rather than half-read.
- **Unknown keys are refused.** A typo in a key name is a startup error instead of a setting that
  silently never applies.
- **Absence changes nothing.** With `TUCANO_CONFIG_FILE` unset no file is consulted at all, so an
  existing environment-only deployment behaves exactly as before. There is deliberately no implicit
  default path — a stray file in a writable directory must not be able to change a deployment
  silently.
- **Nothing is ever written back.** The file is read-only for the process, which keeps the container's
  `read_only: true` root filesystem intact, and the service never creates a configuration file.
- **A secret may appear in the file, but never *only* there** — it must still be reachable from the
  environment, so key material can always come from an orchestrator-managed secret. A secret given
  both inline and by file is refused as a conflict, and one shorter than 32 bytes is refused.
- **Startup errors name the setting and never the value.** No error line, log record, or response
  contains a secret's value, the configuration file's raw path, or its contents.

`TUCANO_DATA_DIR`, `PORT` and `TUCANO_CONFIG_FILE` itself stay environment-only: all three must be
readable *before* the file can be located, and the orchestrator owns all three. Encrypted
configuration files are decided but deferred, and are tracked in
[docs/security/configuration-decision.md](docs/security/configuration-decision.md).

The run scope of a caller is the set of projects a run's `projects` array names, so a caller that may
write a run could narrow its own later scope by editing that array. Tracked as a known weakness in
[docs/security/threat-model.md](docs/security/threat-model.md).

### Errors and request limits

Every rejection the application raises answers the stable envelope
`{"error": {"code": "…", "message": "…"}}`. Published codes are `invalid_id`, `invalid_request`,
`invalid_status`, `invalid_multipart`, `missing_file`, `not_found`, `conflict`, `storage_error`,
`missing_token`, `invalid_token`, `token_expired`, `invalid_credentials`, `invalid_refresh_token`,
and `forbidden`. `openapi.json` names, per operation, the codes that operation can return. A `401`
additionally carries a `WWW-Authenticate: Bearer realm="…"` challenge.

Two answers do not use the envelope, because they come from the router or an extractor rather than from a
handler:

- A request body larger than 50 MiB is refused by the router's size limit with `413` and the plain-text body
  `length limit exceeded`. The limit applies to every route, so it is checked before any handler runs.
- A body the multipart extractor cannot frame — an upload sent as JSON, or without a boundary — is answered
  with `400 text/plain`. Once the framing parses, upload rejections use the envelope.

The full reconciliation of the documented error contract and schema strictness is recorded in
[docs/contracts/api-compatibility.md](docs/contracts/api-compatibility.md).

The API process is stateless: replicas do not keep sessions or in-memory records. Horizontal scaling requires a shared persistent POSIX volume mounted at the same `TUCANO_DATA_DIR` for every replica. Repository mutations use an advisory lock file and atomic same-directory renames. A local Docker volume is suitable for one node; multi-node deployments must provide shared storage with working advisory locks. Do not use separate per-replica local volumes, or data will diverge. The filesystem is the only storage backend: object storage (S3) was declined as a persistence backend by [docs/architecture/adr-object-storage.md](docs/architecture/adr-object-storage.md), which also records the terms under which a bucket may be used as an out-of-process mirror. The backend inventory, what it guarantees, and the backup, scaling and rollback consequences are in [docs/architecture/storage-backends.md](docs/architecture/storage-backends.md).

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

## Test-data generator

`scripts/seed.mjs` is the test-data generator: it builds the full demo environment described by
[docs/testing/seed-dataset-spec.md](docs/testing/seed-dataset-spec.md) against a running deployment,
and `scripts/teardown.mjs` is its inverse. Every project, suite, case, attachment, run, result,
defect, milestone, configuration and import is produced by an HTTP call, so the resulting tree is
always a shape the API itself would write — which makes the dataset a reference for the storage
layout rather than a checked-in fixture that can drift. The one exception is the auth accounts and
grants of spec §5, which the API publishes no route for; they are written through the `AuthStore`
path by the server binary, as [Accounts and grants](#accounts-and-grants) describes.

Configurations, test runs and milestones are project-owned: the generator creates each one through
the project that holds it — `POST /projects/{id}/configurations`, `POST /projects/{id}/test_runs`,
`POST /projects/{id}/milestones` — which is the only way a new one comes into existence. Their bare
document routes (`GET /configurations/{id}`, `GET /test_runs/{id}`, `GET /milestones/{id}`) still
address them by id, and the validation step reads each of the three back through both routes, so a
resource that ends up outside the project the spec gives it fails rather than going unnoticed.

This section is how to **use** the generator. [Building the dataset on a new feature](#building-the-dataset-on-a-new-feature)
is how to **extend** it when the API gains a feature, and the spec is the contract both follow.

### What the generator is for

The same run serves three audiences, and each one has a different entry point. Read the row that
matches what you are doing; the commands are the ones under [Running the generator](#running-the-generator).

| You are | Use the generator to… | Where to start |
| --- | --- | --- |
| **A new user** evaluating or learning Tucano Test | Get a populated deployment in one command instead of clicking through an empty GUI: two projects, suites, cases with steps and attachments, a configuration in each project, two runs with results and defect links, an import, a milestone, a duplicate and a placed copy — enough to see every screen and every report with real content. | [Running the generator](#running-the-generator), then the [storage concept](#storage-concept) above to see the tree it produced under `TUCANO_DATA_DIR`. |
| **A developer** changing the API or the storage layout | Read a fixture that is not committed and cannot drift: the dataset is generated by the same HTTP routes a client calls, so if the storage layout or a document shape changes, the generated tree changes with it. `scripts/teardown.mjs` removes it again, exactly and only. | [Running the generator](#running-the-generator) and [Teardown](#teardown) for the round trip; `docs/testing/seed-dataset-spec.md` §2 for the tree the change should produce. |
| **QA or a reviewer** exercising a running build | Drive the API as a test bed: the spec's §1 coverage matrix is the checklist of features that must have a seeded example, and the spec's step 12 validation is the set of assertions a candidate deployment must satisfy. Use it as a scratch environment to reproduce a report or confirm a fix. | `docs/testing/seed-dataset-spec.md` §1 for the coverage matrix and §3 step 12 for the assertions. Point it at a throwaway deployment, never at one holding data you care about. |

Two properties make the dataset usable as a fixture and a test bed rather than a demo only:

- **It is generated, never stored.** Nothing in `docs/testing/seed-dataset-spec.md` §2's tree is
  hand-written into the data directory, so the fixture is re-derived on every run and a change to
  the API is reflected without an edit here.
- **It is scoped, in both directions.** The seed refuses to run over its own identifiers, and
  teardown removes the seed's entities by identifier and leaves everything else on the volume
  alone, reporting whatever it could not resolve. Neither script needs a dedicated volume, so the
  generator can be pointed at a shared or demo deployment safely.

### Running the generator

It needs Node.js 18+ (native `fetch` and ES modules) and a deployment started **with auth enforced**,
because the sequence signs in as the bootstrap account and creates projects:

```sh
TUCANO_AUTH_REQUIRED=true \
TUCANO_JWT_SECRET='<at least 32 bytes>' \
TUCANO_BOOTSTRAP_USERNAME=admin \
TUCANO_BOOTSTRAP_PASSWORD=admin-password \
  docker compose up -d --build

node scripts/seed.mjs http://localhost:3000
```

Environment:

| Variable | Purpose |
| --- | --- |
| `TUCANO_BOOTSTRAP_USERNAME` / `TUCANO_BOOTSTRAP_PASSWORD` | The account the seed signs in as. Required. |
| `TUCANO_API_URL` | Base URL, when no argument is given. The script otherwise probes `http://localhost:3100`, `http://localhost:8080/api`, then `http://localhost:3000`. |
| `TUCANO_SEED_VIEWER_PASSWORD` | Password for the seeded non-administrator `viewer` account. |
| `TUCANO_SEED_AUTH_CMD` | Command line that seeds the `viewer` account and its grants (see below). When unset, that step is skipped with a notice. |

### Teardown

`scripts/teardown.mjs` removes exactly what `scripts/seed.mjs` created — spec §4's ordered cleanup of
milestones, then runs, then suites and their copies, then cases and their placed copies, then the
two configurations the seed created — each read back from and deleted through the project that owns
it — then the projects that held them, then the seeded account and grants. It needs the same sign-in
credentials the seed used:

```sh
TUCANO_BOOTSTRAP_USERNAME=admin \
TUCANO_BOOTSTRAP_PASSWORD=admin-password \
  node scripts/teardown.mjs http://localhost:3000
```

It is the opposite of `clear-data.mjs`: **removal is by the exact identifiers the seed created**,
never by pattern or "clear the collection". Anything it finds that the seed did not create is named
in the report and left in place, and the run exits non-zero so a deployment holding somebody else's
data is never reported as a clean teardown. Re-running it over an already-clean volume exits 0:
a missing entity is a settled teardown, not a failure.

| Variable | Purpose |
| --- | --- |
| `TUCANO_BOOTSTRAP_USERNAME` / `TUCANO_BOOTSTRAP_PASSWORD` | The account teardown signs in as. Required. |
| `TUCANO_API_URL` | Base URL, when no argument is given. Unlike `clear-data.mjs` this script never guesses: it fails when no candidate answers `/health`. |
| `TUCANO_SEED_VIEWER_USERNAME` | The account to remove. Defaults to `viewer`. |
| `TUCANO_UNSEED_AUTH_CMD` | Command line that removes the account and its grants (see below). When unset, that step is reported as not run and the run exits non-zero, because the account would otherwise survive. |

It exits 0 when everything the seed created is gone or was never there, and 1 when something could
not be resolved or removed — the report names each item it left in place and why.

### Accounts and grants

The API publishes no route that creates an account or records a project grant, so spec §5's second
account cannot be created over HTTP like everything else. Instead the server binary exposes a
subcommand that writes through the very `AuthStore` the running server reads:

```sh
TUCANO_DATA_DIR=/data ./tucano-test seed-auth \
    --username viewer --password viewer-password \
    --grant checkout.json=owner
```

It is idempotent — an account that already exists keeps its password and only the grants it is
missing are added — and it validates each role name before writing anything. Point the seed at it
with `TUCANO_SEED_AUTH_CMD`, for example against the Compose volume:

```sh
TUCANO_SEED_AUTH_CMD='docker compose exec -T api tucano-test seed-auth' \
TUCANO_SEED_VIEWER_PASSWORD=viewer-password \
  node scripts/seed.mjs http://localhost:3000
```

Afterwards the script signs in as `viewer` and asserts `GET /auth/me` reports no system
administrator flag and the `owner` role on `checkout.json` alone — the project it is granted, with
no role at all on `payments.json`. The withheld project is deliberate: a caller who reaches one
project and not the other is what makes the configuration isolation the validation step checks
observable, and the `GET /auth/me` assertion proves the grant files are honoured by the server
rather than merely present.

The inverse is `unseed-auth`, which forgets one named account and the grants that account holds on
the named projects, and nothing else:

```sh
TUCANO_DATA_DIR=/data ./tucano-test unseed-auth \
    --username viewer --grant checkout.json
```

It requires at least one `--grant <project>`, because it removes only the grants it is told about
rather than every grant an account happens to hold. It refuses the bootstrap account outright, and
an account or grant it cannot find is reported as left in place rather than guessed at. It ends with
a machine-readable summary line that `scripts/teardown.mjs` parses to tell an account that was
already gone (`account=absent`, a settled teardown) from one it declined to touch (`account=kept`).
Pass `--keep-account` to remove the grants but leave the account itself. Point teardown at it with
`TUCANO_UNSEED_AUTH_CMD`, for example against the Compose volume:

```sh
TUCANO_UNSEED_AUTH_CMD='docker compose exec -T api tucano-test unseed-auth' \
  node scripts/teardown.mjs http://localhost:3000
```

### Repeated runs

The seed is **not idempotent by design**: the spec fixes the identifiers it creates, and placing a
case onto an identifier the target parent already holds is a conflict. It therefore refuses up front
when its own identifiers are already present and tells you to clear first, rather than failing
half-way through:

```sh
node scripts/teardown.mjs   # then re-run scripts/seed.mjs
```

`teardown.mjs` is the scoped option and is what this repository uses: it removes the seed's own
entities and leaves anything else on the volume alone, reporting what it could not resolve. Use
`clear-data.mjs` only when you deliberately want the whole volume emptied — it removes every
milestone, run, suite, case and project, and does not touch accounts. It has no configuration step
of its own for the reason [Clearing Data](#clearing-data) gives: a configuration is reached through
the project that owns it, so it goes when that project does.

### Scripts in `scripts/`

| Path | Role |
| --- | --- |
| `demo.sh` | The one documented command: brings up a Compose stack, seeds it, runs `smoke.sh`, then validates the dataset with `validate-seed.mjs` |
| `smoke.sh` | Scratch CRUD round trip against a running API, for validating a candidate build |
| `seed.mjs` | Builds the demo dataset of `docs/testing/seed-dataset-spec.md` over HTTP |
| `validate-seed.mjs` | Asserts the seeded dataset through the API, including the refusals a non-admin receives |
| `teardown.mjs` | Removes exactly what `seed.mjs` created, by identifier, and reports anything it leaves in place |
| `check-matrix.mjs` | Fails when a route in `openapi.json` and a row of the dataset's coverage matrix disagree |
| `generate-operations-reference.mjs` | Renders `docs/generated/operations-reference.md` from `openapi.json`; `--check` fails on any drift |
| `check-docs-links.mjs` | Fails when a documentation link breaks, a page is missing from `SUMMARY.md` or the README table, or the wiki index is incomplete |
| `sync-github-wiki.mjs` | Stages `docs/wiki/` as GitHub Wiki pages (`--out <dir>`); `--check` verifies the flat-namespace mapping. The `wiki` CI job publishes the staged pages on `main` |
| `clear-data.mjs` | Unscoped wipe: empties every sample collection a run or seed left behind, with each configuration going as part of the project that holds it |
| `fixtures/` | The small files the seed uploads: a case attachment, a step attachment, and the JUnit report it imports; a new feature that needs a file adds it here |

### Building the dataset on a new feature

The seed is the only place the dataset is defined, so a new API feature reaches the demo environment
through five edits, in this order. The order matters: the spec is the contract, the generator
implements it, and the README describes it — writing the code first is how the two drift apart.

1. **Add the fixture to `scripts/fixtures/`** when the feature needs one, as the seed's attachments
   and JUnit report do. Fixtures are the small files `scripts/seed.mjs` uploads; a feature that needs
   no file skips this step.
2. **Add a matrix row** to `docs/testing/seed-dataset-spec.md` §1 — the row shape is `#`, feature,
   seeded example, producing call, on-disk evidence — and name any new file in §2's target tree, then
   add the calls to §3 in the step whose resources they depend on. A row with no example is a gap,
   not a deferral.
3. **Implement the calls** in `scripts/seed.mjs` as a `stepN…` function, in the order spec §3 gives,
   and add them to the sequence in `runSeed()`. Call the API over HTTP as the other steps do — the
   only permitted exception is spec §5's accounts and grants — and read anything the API derived (a
   duplicate id, a link id) back from a response rather than inventing it, so a `GET` can be asserted
   against what was written.
4. **Extend `scripts/teardown.mjs`** so the run is reversible: add the new identifiers to the
   constants at the top and a removal to the step whose dependencies allow it. Every removal goes
   through the guard mechanism, and anything the teardown cannot resolve is reported as kept rather
   than guessed at.
5. **Assert it in the validation step** of spec §3 step 12, so the new row fails loudly instead of
   going stale. This is the check the spec's stale-matrix paragraph promises; it is implemented by
   the Compose wiring and freshness check of the epic (#195).

Two habits keep the result honest, both recorded in spec §6: a new feature adds a matrix row, a new
row adds an assertion, and a new stored file is named in the target tree. A file the seed writes but
the spec does not name is a bug in one of the two.

A worked example is matrix rows 14 and 15 — the recorded results (every status, and the replacement
of one). They needed no fixture, one row each, two `POST /test_runs/{id}/results` calls in
`step7Results`, and no teardown change beyond the run that already owns them, because a run and its
results are removed with the run.

### Validating a seeded deployment

The generator's validation step is the acceptance check for the dataset: it asserts `GET /health`
and `GET /ready`, reads every document of the target tree back through its `GET` route, checks that
each project-owned resource — configurations, runs and milestones — also reads back through the
listing of the project that owns it (and not through another project's), checks
`GET /milestones/v1.0.json/progress` reports five buckets with `totalCases` equal to the cases the run
declares, exercises both report scopes and the `?tags=` and `?configuration=` filters, reads
`GET /auth/me`, confirms the seeded viewer reaches one project and not the other, and confirms a
guarded write with an under-privileged token answers `403 forbidden`.
The full list is spec §3 step 12; run it after seeding a candidate build, in the same scratch
deployment, before promoting anything.

## Test Data Cleanup

Three helper scripts in `scripts/` remove sample data against a running API instance, and they are
deliberately not interchangeable:

- `teardown.mjs` removes exactly what `scripts/seed.mjs` created, by the identifiers the seed fixed.
  Anything else on the volume is left alone and named in its report. Use this when you want the demo
  environment gone and everything else preserved — see [Teardown](#teardown).
- `clear-data.mjs` wipes the whole collections — every milestone, run, suite, case and project, with
  each configuration going as part of the project that holds it. Use it when you want the volume
  emptied and do not care what else was in it.
- `smoke.sh` creates and deletes only its own scratch round trip, so it needs no cleanup step at all.

### Prerequisites

Node.js 18+ (uses native `fetch` and ES modules). Teardown additionally needs the bootstrap
credentials, because it signs in rather than calling anonymously.

### Tearing down the seed

```sh
TUCANO_BOOTSTRAP_USERNAME=admin \
TUCANO_BOOTSTRAP_PASSWORD=admin-password \
  node scripts/teardown.mjs http://localhost:3000
```

It exits 0 when the seed's entities are gone or were never there, and 1 when something could not be
resolved — naming every item it left in place and why. See [Teardown](#teardown) for the environment
variables, including `TUCANO_UNSEED_AUTH_CMD`, which is needed for the seeded account to be removed
as well.

### Clearing Data

Wipes all milestones, test runs, test suites, projects, and test cases (with their attachments) from
the API. With no argument the script probes `http://localhost:3100`, then `http://localhost:8080/api`,
then `http://localhost:3000`, and uses the first base URL that answers `/health`, so a Compose stack
is found without arguments.

**Configurations are not removed by a step of their own**, and the script says so in its header
comment. A configuration is a project resource: it is reached through the project that owns it, so a
bare `DELETE /configurations/{id}` would not say which project's copy was meant, and this script's job
is to clear everything rather than to guess at a home. It does not have to guess: deleting a project
cascades to the configurations, runs and milestones inside it, so every configuration that lives in a
project the script lists goes with that project — and it deletes every project it lists.
`scripts/teardown.mjs` is the scoped opposite: it removes exactly the configurations the seed created,
each read back from and deleted through the project that holds it. Accounts and grants are still left
alone by both, because the API publishes no route for them.

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
| `tests/service.rs` | Health, readiness and storage diagnostics, OpenAPI document and its error contract, Swagger UI, malformed bodies, traversal rejection, the identifier and size-limit error answers, the on-disk tree layout, persistence across restarts |
| `tests/projects.rs` | Project CRUD, validation, conflicts, error envelopes |
| `tests/suites.rs` | Test suite CRUD, parent-scoped creation, copy/move composition, ambiguity conflicts, missing resources |
| `tests/runs.rs` | Test run CRUD, validation, conflicts, missing resources, the case-version capture each run records, JUnit XML and JSON result import, and listing, linking and unlinking the defect links a result carries |
| `tests/cases.rs` | Test case CRUD, required fields, parent-scoped creation, copy/move composition, conflicts, missing resources, and versioning — the `version`/`lastModified` stamp, the `revisions/` snapshots a qualifying update writes, and the history endpoints that list and read them back |
| `tests/milestones.rs` | Milestone CRUD, validation, conflicts, duplication, and progress derived from the referenced runs |
| `tests/configurations.rs` | Configuration CRUD, validation, conflicts, missing resources, restart persistence, and use by a run |
| `tests/reports.rs` | The reports: the coverage report (per-suite and total case counts, the project scope filter, the global scope) and the run summary (the status buckets, the pass rate, the summed durations, the intersecting project/milestone/configuration and date filters), with the error answers for an unknown and an unusable identifier |
| `tests/request_id.rs` | The request id: the minted `X-Request-Id` on a request that sends none, the verbatim echo of an inbound one, the replacement of an empty header, distinct ids per request, the `requestId` the error envelope carries, the header a plain-text rejection still carries, and the id the request span is given |
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
| [docs/architecture/adr-object-storage.md](docs/architecture/adr-object-storage.md) | ADR: why object storage (S3) is declined as a persistence backend and the file-based invariant is upheld (#181) |
| [docs/architecture/storage-backends.md](docs/architecture/storage-backends.md) | The storage backends and their operational implications (#186): the one implemented backend, what it guarantees, backup/scaling/rollback consequences, and the declined object-store backend versus the permitted external mirror |
| [docs/architecture/adr-storage-layout-v3.md](docs/architecture/adr-storage-layout-v3.md) | ADR: storage layout v3 (#215) — runs, milestones and configurations live inside their project and are governed by it |
| [docs/architecture/wiki-structure-and-publication.md](docs/architecture/wiki-structure-and-publication.md) | The wiki decision: source of truth, publication mechanism, page inventory, and the drift-prevention rule |
| [docs/contracts/api-compatibility.md](docs/contracts/api-compatibility.md) | File-format and endpoint compatibility rules against the legacy implementation |
| [docs/contracts/test-case-versioning-plan.md](docs/contracts/test-case-versioning-plan.md) | Field names, snapshot shape, trigger rules, and addressing for test-case versioning and revision history |
| [docs/contracts/file-format-versioning-plan.md](docs/contracts/file-format-versioning-plan.md) | The `formatVersion` storage marker: field, reader and writer rules, migration rules, and the rollback drill matrix |
| [docs/deployment/deployment-guide.md](docs/deployment/deployment-guide.md) | The deployment model: the JSON volume mount, Compose configuration, the optional configuration file, container hardening, scaling, and rollback to an immutable release tag |
| [docs/deployment/config.example.json](docs/deployment/config.example.json) | The configuration-file template, kept valid against the loader's schema by a unit test |
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
| [docs/testing/seed-dataset-spec.md](docs/testing/seed-dataset-spec.md) | The demo/seed dataset: the feature coverage matrix, the target tree below `TUCANO_DATA_DIR`, the API calls that produce it, and the teardown scope |
| [docs/testing/test-data-generator-guide.md](docs/testing/test-data-generator-guide.md) | Using the test-data generator as a demo, a fixture and a test bed, and how to extend it when a feature lands (#196) |
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
