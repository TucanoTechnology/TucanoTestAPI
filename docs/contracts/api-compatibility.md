# API Compatibility Contract

## Status

The Rust service implements CRUD and attachment endpoints, and `openapi.json` is checked in and served at `/api-docs`.

The legacy [TucanoTest](https://github.com/ECiurleo/TucanoTest) implementation remains the behavioural authority for file formats. The legacy Draft 2020-12 schemas set `additionalProperties: false`, so adding any field is a breaking change and requires an explicit versioning plan recorded in this document before implementation.

Sanitised reference fixtures have **not** yet been captured, so cross-implementation contract tests are still outstanding.

## Fixture layout

When the reference API is available, store sanitized fixtures under `compatibility/` using this layout:

```text
compatibility/
  endpoints/<resource>/<operation>-<case>.json
  responses/<resource>/<operation>-<case>.json
  persistence/<resource>/<operation>-<case>.json
  openapi.json
```

Fixtures must not contain credentials, tokens, personal data, production paths, or real attachment contents.

## Required observation record

Each endpoint case must record:

- HTTP method and path
- Query, path, and header inputs
- Request body and content type
- Authentication and authorization context
- Status code and response headers
- Response JSON shape and error envelope
- Request ID behavior and correlation headers
- Filesystem files changed, including exact JSON format
- Repeat-request and concurrent-request behavior

## Compatibility rules

1. Preserve status codes, response shape, required headers, and documented error codes unless a deviation is approved.
2. Preserve JSON field names, types, nullability, date formats, and omission behavior.
3. Preserve file names and JSON formats during the evaluation; incompatible changes require explicit versioning.
4. Compare persistence effects as well as HTTP responses.
5. The GUI consumes this same HTTP contract and never accesses storage directly.
6. Internal paths, stack traces, raw filesystem errors, secrets, and file contents must never appear in client errors or logs.
7. Rust deviations must be listed with rationale, migration impact, and a test proving the new behavior.

## Test Run Results Schema Versioning Plan (Issue #22)

To record per-case execution outcomes within test runs without breaking existing run files:
- `TestRun` JSON is extended with an optional `results` array of `TestCaseResult` objects.
- Each `TestCaseResult` includes:
  - `testCaseId`: string (required)
  - `status`: string (required: `Passed`, `Failed`, `Blocked`, `Untested`, or `Retest`)
  - `timestamp`: ISO-8601 string (required)
  - `notes`: string (optional)
  - `attachments`: array of `Attachment` objects (optional)
- Existing run JSON files omitting `results` remain valid and deserialize with `results: None`.
- Serializing `TestRun` instances with `results: None` omits the field, preserving legacy file compatibility.

## Test Suite Composition Plan (Issue #23)

To allow building test suites incrementally while preserving legacy fixture compatibility:
- `TestSuite` JSON continues embedding full `TestCase` objects in `testCases` to remain compatible with legacy Draft 2020-12 schemas.
- `POST /test_suites/{id}/test_cases` (payload `{"testCaseId": "TC-001"}`) resolves the specified test case from repository storage, validates that it is not already present in the suite, and appends the case to `testCases`.
- `DELETE /test_suites/{id}/test_cases/{case_id}` removes the matching test case from `testCases` by ID.
- Requests referencing unknown test cases return `404 Not Found`.
- Requests adding duplicate test cases to the same suite return `409 Conflict`.

## Test Run Composition & Execution Plan (Issue #24)

To allow assembling runs from suites and cases, and recording per-case execution results:
- `POST /test_runs/{id}/test_suites` (payload `{"suiteId": "Smoke.json"}`) resolves the suite from repository storage and appends it to `run.testSuites`.
- `POST /test_runs/{id}/test_cases` (payload `{"testCaseId": "TC-001.json"}`) resolves the test case from repository storage and appends it to `run.testCases`.
- `POST /test_runs/{id}/results` (payload `{"testCaseId": "TC-001.json", "status": "Passed", "notes": "notes"}`) records or updates a `TestCaseResult` in `run.results`.
- Results persist in `run.results` in run storage and never mutate source `TestCase` or `TestSuite` files.
- Recording results for the same test case across multiple runs maintains total isolation between runs.

## Hierarchy and Real-Home Storage Plan (Issue #65)

Issue: [#65](https://github.com/TucanoTechnology/TucanoTestAPI/issues/65) — storage layout must mirror the
conceptual organisation so a project, suite, or case has one real home and supplementary files live with the
entity. This plan supersedes the flat-layout behaviour implied by the baseline: `projects/`, `test_suites/`, and
`test_cases/` are no longer flat sibling collections.

### v2 layout

```text
<data>/
  projects/
    <project>/
      project.json                     project metadata marker
      <test suite>/
        suite.json                     suite metadata marker
        <test case>/
          test-case.json               full case document (legacy shape)
          <attachment files>
      <test case>/                     case owned directly by the project
        test-case.json
        <attachment files>
  test_runs/<id>.json                  flat; point-in-time snapshot copies
  milestones/<id>.json                 flat
  configurations/<id>.json             flat (activated by Issue #69)
```

- A project folder is named after the project wire id minus its trailing `.json` (id `checkout.json` →
  folder `checkout`). Suite folders follow the same rule inside their project folder.
- A case folder is named after the `testCaseId` verbatim (bare ids stay bare; `.json`-suffixed ids keep the
  suffix), matching today's `test_cases/<id>/test-case.json` convention.
- Folder presence is membership: a project's suites are the suite folders inside it, a suite's cases are the case
  folders inside it, and a project's directly owned cases are the case folders directly inside the project
  folder. Membership is never duplicated inside parent documents.
- Parent markers keep the legacy Draft 2020-12 field shapes with **empty child arrays**: `project.json` stores
  `testSuites: []` and `suite.json` stores `testCases: []`. Expanded content is assembled on read from the
  physical folders (see *Reads* below).
- Case documents are stored whole; a case's `attachments` metadata array is kept accurate by the attachment
  endpoints (upload appends, delete removes), so it always reflects the files actually stored in the case folder.

### Identity rules

- Project ids stay globally unique. Suite ids are unique **within a project**, case ids are unique **within a
  parent** (project or suite). A suite id may therefore exist under several projects, and a case id under several
  parents (the result of copy-on-include).
- Children of one parent share a single folder namespace: a suite base and a direct-case folder name cannot
  collide, and creating a second child with an existing name returns `409 Conflict`.
- Creating a child whose name equals a parent marker file name (`project.json` in a project, `suite.json` in a
  suite) collides with the marker file and returns `409 Conflict`.

### Reads and writes

- `GET /test_cases/{id}`: unique case folder is returned; the stored document is the response.
- `GET /test_suites/{id}`: the suite marker is returned with `testCases` replaced by the assembled child case
  documents (ordered by folder name).
- `GET /projects/{id}`: the project marker is returned with `testSuites` replaced by the assembled child suites
  (each recursively assembled) and, when the project owns at least one direct case, an additional **new optional
  `testCases` field** holding the assembled directly owned cases. The field is omitted when there are none so the
  legacy response shape is preserved in the common case. (Wire addition — see *Breaking change accounting*.)
- `GET /test_suites` and `GET /test_cases` remain global scans of the tree and return distinct, sorted ids.
- Global dereference (`GET`/`PUT`/`DELETE /test_suites/{id}`, `/test_cases/{id}`, attachment and duplicate
  routes): zero occurrences → `404 Not Found`, exactly one → operate, two or more → `409 Conflict` with a
  message directing the caller to the parent-scoped endpoints. Lists never fail on duplicates; they de-duplicate.
- Create and update payloads for projects, suites, and cases are validated exactly as today. Child arrays in a
  parent payload (`testSuites`/`testCases`) are accepted for wire compatibility but never persisted — markers
  always store empty arrays and every read re-assembles membership from folders, so stored parents can never
  become stale.
- Delete cascades follow the tree: deleting a project removes its whole subtree; deleting a suite removes its
  child cases and their attachments; deleting a case removes its folder and attachments. Runs hold point-in-time
  copies, so cascades never corrupt previously recorded runs, and milestone progress simply recomputes over the
  runs that still exist.

### Attachments

- Attachments are stored as sibling files inside the case folder (one case = one folder, wherever that case
  lives). Upload/download/delete act on the resolved occurrence; an ambiguous case id returns `409 Conflict`.
- The case marker's `attachments` array is updated under the same storage lock as the file operation so metadata
  and files never diverge for API-mediated changes.

### Storage security invariants

All baseline invariants remain: every identifier passes `validate_component`; every constructed path passes
`ensure_within`; parent and child paths are depth-checked; writes are atomic same-directory temp files with
`0o666` permissions; raw paths or filesystem errors never reach clients.

## Real-Parent Creation Plan (Issue #66)

Issue: [#66](https://github.com/TucanoTechnology/TucanoTestAPI/issues/66) — creating a suite or case requires a
real parent; reads and global lists stay global.

- Suites are created with `POST /projects/{project_id}/test_suites` (payload requires a non-empty `name`, as
  today). `POST /projects` continues to create projects.
- Cases are created with `POST /projects/{project_id}/test_cases` or `POST /test_suites/{suite_id}/test_cases`
  (payload requires `testCaseId`, `title`, and `expectedResult`, as today). A case created under a suite is owned
  by that suite; a case created directly under a project is owned by that project.
- The flat top-level creation endpoints `POST /test_suites` and `POST /test_cases` are retired: the router no
  longer registers them and `openapi.json` documents only the parent-scoped creation. The response when a caller
  needs a stable explanation is `400 Bad Request` with the standard error envelope and a message naming the
  replacement route.
- New parent-scoped collection routes, all documented in `openapi.json`:
  - `GET /projects/{project_id}/test_suites` — the project's suite ids.
  - `GET /projects/{project_id}/test_cases` — the project's direct-case ids.
  - `GET /test_suites/{suite_id}/test_cases` — the suite's member case ids.
  - `DELETE /projects/{project_id}/test_suites/{suite_id}` and
    `DELETE /projects/{project_id}/test_cases/{case_id}` — delete that occurrence (cascade semantics of #65).
- Success responses remain `201 Created` with `{"message", "id"}` and create the physical folder immediately.
- Unknown parent → `404 Not Found`; duplicate child id within the parent → `409 Conflict`; missing required
  fields → `400 Bad Request` with the existing field matrix.

## Copy/Move Include Semantics Plan (Issue #67)

Issue: [#67](https://github.com/TucanoTechnology/TucanoTestAPI/issues/67) — placing an existing entity into a
parent supports both duplicate-on-include and move semantics. This supersedes the Issue #23 composition notes for
`POST /test_suites/{id}/test_cases` (which described an implicit copy) and clarifies Issue #24 run composition.

### Inclusion payloads

Placement uses the same collection routes as creation, disambiguated by payload grammar:

- `POST /projects/{project_id}/test_suites` with a non-empty `name` creates a new suite; with `suiteId` (and no
  `name`) it places an existing suite into the project.
- `POST /projects/{project_id}/test_cases` and `POST /test_suites/{suite_id}/test_cases` with `title` **and**
  `expectedResult` create a new case; with only `testCaseId` (and no `title`) they place an existing case.
- Placement bodies accept an optional `"mode": "copy" | "move"`; the default is `copy`. A body mixing creation
  fields and `mode` is rejected with `400 Bad Request`.
- `copy` duplicates the source subtree into the target parent (files, nested cases, and attachments included).
  The copy is fully independent: deleting either occurrence leaves the other intact. `move` physically relocates
  the folder, so the old parent loses membership. If the target parent already has a child with the same id, both
  modes return `409 Conflict`.
- Moving/copying a suite targets a project; moving/copying a case targets a project or a suite. Runs are never
  targets of placement: `POST /test_runs/{id}/test_suites` and `POST /test_runs/{id}/test_cases` always embed an
  assembled snapshot copy (Issue #24 semantics) and never take ownership.

### Effect on Issue #23 endpoints

`POST /test_suites/{suite_id}/test_cases` is now the suite-scoped create/place endpoint described above, and
`DELETE /test_suites/{suite_id}/test_cases/{case_id}` removes that occurrence from the suite (deleting the
occurrence's folder under the cascade rules of #65; other occurrences elsewhere are untouched). Because runs
store copies, no recorded run is affected.

## Milestone Duplicate Plan (Issue #68)

Issue: [#68](https://github.com/TucanoTechnology/TucanoTestAPI/issues/68)

- `POST /milestones/{id}/duplicate` mirrors the other duplicate endpoints: reads the source milestone, applies an
  optional `newId` override, else derives `{id base}-copy-{nanos}`, retains `name`, dates, `status`,
  `testSuiteIds`, and `testRunIds`, and writes the copy. Progress is derived, so the copy reports the same
  progress as the source while both reference the same runs.
- Unknown source → `404 Not Found`; existing target id → `409 Conflict`; success → `201 Created` with
  `{"message", "id"}`.

### Duplicate id normalisation (parity fix)

Project, suite, run, and milestone duplicates currently derive a new id from the document id field without the
required `.json` suffix, so an auto-derived duplicate is rejected by storage (`400 Bad Request`). Deviation
fix, with tests: file-backed resources (`projects`, `test_suites`, `test_runs`, `milestones`) derive
`{wire id base}-copy-{nanos}.json`; `test_cases` keep bare ids. The document id field is set to the new id only
when no `newId`/`newName`/`newTitle` override is present, preserving the legacy observable behaviour.

## Configurations Activation Plan (Issue #69)

Issue: [#69](https://github.com/TucanoTechnology/TucanoTestAPI/issues/69)

- The `configurations` resource is added to the repository resource set so the already-registered CRUD routes
  stop returning `400 Invalid request` from the storage layer.
- `/configurations` and `/configurations/{id}` are documented in `openapi.json` with the same CRUD semantics as
  the other flat resources; `tests/service.rs` route coverage is extended accordingly. Full environment-matrix
  semantics remain out of scope here and stay tracked by Issues #34 and #51.

## API Handler Decomposition Plan (Issue #70)

Issue: [#70](https://github.com/TucanoTechnology/TucanoTestAPI/issues/70)

- `src/api.rs` is decomposed into a `src/api/` module directory: `mod.rs` keeps the router, shared state, and the
  shared validation/error helpers; per-resource modules hold projects, suites, cases, runs, milestones,
  configurations, and attachments handlers.
- This is a pure structural refactor: no route, payload, or behaviour change; `lib.rs` continues to expose
  `pub mod api;` and the full test suite passes unchanged at that merge point. No file-format impact.

## Breaking change accounting

- **New optional `testCases` on assembled project responses** (Issue #65). Legacy Draft 2020-12 `Project`
  documents do not know this field; it appears only when a project directly owns cases. The GUI is updated to
  read it; legacy clients that reject unknown fields fail loudly only for projects with direct cases, which
  previously could not exist through this API.
- **Storage layout v2** replaces the flat `projects/`, `test_suites/`, `test_cases/` layout. Existing data
  directories created by earlier builds are development artifacts and are not migrated; fresh layout is created
  on startup. Persistence behaviour is asserted by repository unit tests that inspect the physical tree, and by
  the integration suites.
- **Parent-required creation and copy/move placement** change the composition wire contract described by the
  Issue #23/#24 notes above; those notes remain valid only for the payload shapes that this plan keeps.

## Required case matrix

| Case | Expected evidence |
| --- | --- |
| Create valid resource | Success status, response, and persisted JSON |
| Read existing resource | Success status, headers, and exact representation |
| List resources | Ordering, pagination, empty result, and limits |
| Update valid resource | Replacement/merge semantics and persistence |
| Delete existing resource | Status and missing-resource behavior |
| Missing resource | Status and stable error envelope |
| Missing required field | Validation status and field details |
| Unknown field | Reject, ignore, or preserve behavior |
| Malformed JSON | Parse error status and safe response |
| Unauthorized request | Authentication status and response shape |
| Forbidden request | Authorization status and response shape |
| Conflict/concurrent update | Conflict behavior and file integrity |
| Traversal or symlink path | Rejection without access outside data root |
| Oversized request or attachment | Bounded rejection without unbounded allocation |

## Definition of done for the baseline

- Reference endpoint observations are captured in sanitized fixtures.
- A Swagger/OpenAPI document is checked in and matches the observed routes.
- Contract tests compare Node and Rust status, headers, bodies, and persistence effects.
- Negative security cases cover every threat in [the threat model](../security/threat-model.md).
- Every approved deviation is documented before migration work begins.
