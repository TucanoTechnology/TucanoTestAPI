# Storage Concept and API Reference

This document is the detailed reference for the Tucano Test domain model, HTTP API surface,
authentication, configuration, error contract, and scaling behaviour. The [repository
README](../../README.md) keeps a short overview of each area for developers; this page holds the
full specification.

## Storage concept

Tucano Test is a **file-based test case management system**: there is no database. All state is
kept as folders and JSON files on the filesystem, and everything is managed through the HTTP API —
create, read, update, delete, and duplicate. The GUI and the API are equal citizens: every GUI
action has an API equivalent, and no client ever touches the storage directory directly.

The folder layout mirrors the conceptual organisation of the domain. Each entity is a folder that
contains a JSON file with its details plus any supplementary files that belong to it. It is
described below, and a running deployment writes a working example of all of it that you can read
back: see the [test-data generator](../../README.md#test-data-generator).

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
model instead of one that fails to load. The configuration identity is the one exception: it is
always the derived id, because a `configId` is resolved to the file that holds it, and a stored
value that disagreed with its own document would name nothing (Issue #288). The rules are recorded
in [docs/contracts/api-compatibility.md](../contracts/api-compatibility.md).

### Domain hierarchy

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

### API composition semantics

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
  duplicate) operates on the one occurrence when it is unique and answers `409 Conflict`, naming
  the parent-scoped routes, when it is ambiguous. The attachment routes resolve the same way on
  their bare form, and each of them also has a parent-scoped mirror that names the holder, so an
  attachment of an ambiguous case stays reachable. The global document routes
  for runs, milestones and configurations resolve the same way. Listing routes never fail on
  duplicates; they de-duplicate.

This concept is enforced for agent work in [AGENTS.md](../../AGENTS.md); the storage layout it
describes is recorded in
[docs/architecture/adr-storage-layout-v3.md](../architecture/adr-storage-layout-v3.md).

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

Each attachment family exists in three forms: bare (`/test_cases/{id}/…`), through the holding
project (`/projects/{id}/test_cases/{case_id}/…`) and through the holding suite
(`/test_suites/{id}/test_cases/{case_id}/…`). Only the case attachment has a download route, and it
has one in all three forms; a step attachment is uploaded, listed and deleted, never downloaded.
A download answers the stored bytes as `application/octet-stream`, whatever the file is, with
`Content-Disposition: attachment` naming the file the uploader supplied; the `mimeType` recorded in
the case document is metadata and never becomes the response content type.

List endpoints share `?filter=`, `?tags=` (matched as an OR set), and — for runs, the only
collection with configuration references — `?configuration=`. The retired flat creation routes
(`POST /test_suites`, `POST /test_cases`, `POST /test_runs`, `POST /milestones`,
`POST /configurations`) are still registered, but only to answer `400 Bad Request` with the route
that replaced them. The bare global scans (`GET /test_suites`, `GET /test_cases`, `GET /test_runs`,
`GET /milestones`, `GET /configurations`) also stay served for compatibility and are deliberately
absent from `openapi.json`; the documented way to list a project's runs, milestones and
configurations is the parent-scoped route.

## Authentication

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
| `TUCANO_CONFIG_KEY_FILE` | — | Path to the key ring file for decrypting AEAD-encrypted secrets in the configuration file. Environment-only; unset means no encryption keys. |
| `TUCANO_LOCK_TIMEOUT_MS` | `5000` | Milliseconds a write waits for the advisory lock before it is refused with `503 lock_timeout`. Environment-only; must be a whole number, and anything else stops startup. |
| `TUCANO_AUTH_REQUIRED` | `false` | Require and enforce a bearer token on every guarded route. When off, every guard returns and the API is anonymous. |
| `TUCANO_JWT_SECRET` | — | The HS256 signing secret. Required when auth is on; at least 32 bytes. |
| `TUCANO_JWT_SECRET_FILE` | — | A file to read the secret from. Set this **or** `TUCANO_JWT_SECRET`, never both. |
| `TUCANO_ACCESS_TOKEN_TTL` | `15m` | Access-token lifetime; accepts a duration such as `900s` or `15m`. |
| `TUCANO_REFRESH_TOKEN_TTL` | `14d` | Refresh-token lifetime. Refresh tokens are rotated on every use. |
| `TUCANO_BOOTSTRAP_USERNAME` | — | A `systemAdmin` account created at startup when the store held no accounts. Set with `TUCANO_BOOTSTRAP_PASSWORD`. |
| `TUCANO_BOOTSTRAP_PASSWORD` | — | The password for the bootstrap account, stored only as an Argon2id hash. |

The defaults above are the **service's** built-in defaults. The shipped `docker-compose.yml`
overrides one of them: it sets `TUCANO_AUTH_REQUIRED=true` and takes `TUCANO_JWT_SECRET` and the
bootstrap pair from `.env`, so the local Compose stack is authenticated while a deployment that
supplies its own container definition starts anonymous.

Accounts and grants live beside the data, under `TUCANO_DATA_DIR/auth/`; the deployment stays
database-free. The decision of record is
[docs/security/authentication-decision.md](../security/authentication-decision.md) and the
enforcement matrix lives in `tests/auth.rs`.

### Configuration file

Every setting above can also be supplied by an optional JSON file named by `TUCANO_CONFIG_FILE`.
The file is read **once at startup**, before the listener binds, into an immutable value: a file the
server cannot read, cannot parse, or does not recognise is a startup failure, never a request
failure and never a silent fallback. The model and its rationale are in
[docs/security/configuration-decision.md](../security/configuration-decision.md); the template is
[docs/deployment/config.example.json](../deployment/config.example.json):

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

`TUCANO_DATA_DIR`, `PORT`, `TUCANO_CONFIG_FILE` and `TUCANO_CONFIG_KEY_FILE` stay
environment-only: all four must be readable *before* the file can be located or its secrets
decrypted, and the orchestrator owns all four. See
[docs/security/configuration-decision.md](../security/configuration-decision.md) for the
encrypted-secrets model and key rotation.

The run scope of a caller is the set of projects a run's `projects` array names, so a caller that may
write a run could narrow its own later scope by editing that array. Tracked as a known weakness in
[docs/security/threat-model.md](../security/threat-model.md).

## Errors and request limits

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
  with `400 text/plain`. Once the framing parses, upload rejections use the structured envelope
  (`missing_file`, `invalid_multipart`).

The full reconciliation of the documented error contract and schema strictness is recorded in
[docs/contracts/api-compatibility.md](../contracts/api-compatibility.md).

## Scaling and statelessness

The API process is stateless: replicas do not keep sessions or in-memory records. Horizontal scaling
requires a shared persistent POSIX volume mounted at the same `TUCANO_DATA_DIR` for every replica.
Repository mutations use an advisory lock file and atomic same-directory renames. A write that
cannot take the lock within `TUCANO_LOCK_TIMEOUT_MS` (default `5000` ms) is refused with
`503 lock_timeout` and a `Retry-After`, so a busy volume is answered rather than queued behind
indefinitely. A local Docker volume is suitable for one node; multi-node deployments must provide
shared storage with working advisory locks. Do not use separate per-replica local volumes, or data
will diverge. The filesystem is the only storage backend: object storage (S3) was declined as a
persistence backend by
[docs/architecture/adr-object-storage.md](../architecture/adr-object-storage.md), which also records
the terms under which a bucket may be used as an out-of-process mirror. The backend inventory, what
it guarantees, and the backup, scaling and rollback consequences are in
[docs/architecture/storage-backends.md](../architecture/storage-backends.md).

To validate a candidate build before it serves traffic, and to roll back to an previous build
safely, follow
[docs/deployment/canary-validation-and-rollback.md](../deployment/canary-validation-and-rollback.md).

The full deployment model — the JSON volume mount that is the only state, the Compose configuration
and container hardening, single-node versus shared-storage scaling, and rollback to an immutable
release tag — is in
[docs/deployment/deployment-guide.md](../deployment/deployment-guide.md).

## Smoke validation

`scripts/smoke.sh` exercises a running API with a scratch CRUD round trip — health, create a project
and a case, read both back, delete both, and confirm each deletion is observable — and exits non-zero
on the first deviation. It needs `curl` and `python3`, removes its scratch data on exit, and accepts
a base URL. With none given it probes `http://localhost:3100`, then `http://localhost:3000`, and uses
the first that answers `/health`, so a Compose stack is found without arguments:

```sh
scripts/smoke.sh                       # 3100 if it answers, else 3000
scripts/smoke.sh http://localhost:3100 # the Compose api service
scripts/smoke.sh http://localhost:3101 # any replica, for example a canary
```

A base URL that *is* given is used as it stands, so a wrong port is reported rather than quietly
corrected.

Use it to validate a candidate build before promotion; the surrounding procedure is in
[docs/deployment/canary-validation-and-rollback.md](../deployment/canary-validation-and-rollback.md).

## Test data cleanup

Three helper scripts in `scripts/` remove sample data against a running API instance, and they are
deliberately not interchangeable:

- `teardown.mjs` removes exactly what `scripts/seed.mjs` created, by the identifiers the seed fixed.
  Anything else on the volume is left alone and named in its report. Use this when you want the demo
  environment gone and everything else preserved — see the [test-data generator
  guide](../testing/test-data-generator-guide.md).
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
resolved — naming every item it left in place and why. See the [test-data generator
guide](../testing/test-data-generator-guide.md) for the environment variables, including
`TUCANO_UNSEED_AUTH_CMD`, which is needed for the seeded account to be removed as well.

### Clearing data

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
