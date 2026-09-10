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
- `GET /test_suites` and `GET /test_cases` remain global scans of the tree and return distinct, sorted ids. Both
  scans are served but undocumented in `openapi.json` (retired with the flat creation routes — see *Real-Parent
  Creation Plan*), so an old caller can still enumerate the tree while the published contract advertises only the
  parent-scoped routes.
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
- The flat top-level creation endpoints `POST /test_suites` and `POST /test_cases` are retired. The router still
  registers the two paths so an old caller receives a stable explanation, but each answers `400 Bad Request` with
  the standard error envelope and a message naming the replacement route, and **`openapi.json` documents only the
  parent-scoped creation**. Because the published contract is compared path-by-path against the registered route
  set, the exemption is path-granular: the whole `/test_suites` and `/test_cases` path keys are omitted, so the
  global `GET /test_suites` and `GET /test_cases` scans described under Issue #65 are served but undocumented.
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
- Implementation note: `POST /milestones/{id}/duplicate` is published in `openapi.json` through the
  `x-duplicate-milestone` component, and the `tests/service.rs` route coverage includes the path. The parity fix
  lives in `Resource::id_requires_json_suffix()` (`src/storage/layout.rs`), which the shared duplicate helper
  consults when it derives an identifier; the milestone operation accepts `newId` only, because the contract
  above enumerates no renaming override and the copy must retain the source `name`. Coverage:
  `src/domain/duplicate.rs` asserts the derived shape for all four file-backed resources and the bare test-case
  id, `tests/projects.rs` proves the fix end-to-end over HTTP, and `tests/milestones.rs` covers the new
  endpoint's lifecycle, the independence of the two documents, the derived progress, `409 Conflict`, and
  `404 Not Found`.

## Configurations Activation Plan (Issue #69)

Issue: [#69](https://github.com/TucanoTechnology/TucanoTestAPI/issues/69)

- The `configurations` resource is added to the repository resource set so the already-registered CRUD routes
  stop returning `400 Invalid request` from the storage layer.
- `/configurations` and `/configurations/{id}` are documented in `openapi.json` with the same CRUD semantics as
  the other flat resources; `tests/service.rs` route coverage is extended accordingly. Full environment-matrix
  semantics remain out of scope here and stay tracked by Issues #34 and #51.
- Implementation note: the resource-set change and the two documented paths landed with the layered
  architecture refactor (Issue #71); `TestConfiguration` is published in `openapi.json` and
  `tests/configurations.rs` covers the CRUD lifecycle, the error matrix, restart persistence, and a run
  referencing a configuration.

## Layered Architecture Plan (Issue #71)

Issue: [#71](https://github.com/TucanoTechnology/TucanoTestAPI/issues/71) — supersedes the Issue #70 handler
decomposition plan, which is closed as superseded. #71 is the first step of the combined P0 refactor.

- The crate is layered bottom-up, and each layer depends only on the layer beneath it:
  - `src/models.rs` — the serialisable documents, including the legacy camelCase shapes.
  - `src/storage/` — the `Repository` trait and its `FileRepository` implementation, the on-disk layout, atomic
    writes, permissions, and the identifier/filename conventions (`layout.rs`, `fs.rs`). This is the only module
    that touches the filesystem.
  - `src/domain/` — every business rule: payload validation, identifier derivation, suite/run composition,
    duplication and milestone progress, behind `TestService`. Unit tested without starting a server.
  - `src/api/` — one module per resource (`projects`, `suites`, `runs`, `cases`, `milestones`, `configurations`),
    plus `crud.rs` for the handlers every resource shares and `error.rs` for the response envelope. A handler only
    translates a request, calls `TestService`, and shapes the response.
- `src/api.rs` no longer exists. `src/repository.rs` remains as a compatibility shim that re-exports
  `FileRepository`, `Repository`, and `Resource` from `src/storage/`.
- Failures are a single typed `DomainError` mapped onto the stable `{"error":{"code","message"}}` envelope. The
  status codes and error codes issued for existing cases are unchanged.
- The public crate surface is unchanged: `api::router`, `api::MAX_BODY_BYTES`, `api::ListQuery`, `models`, and the
  `repository` re-exports keep their signatures, so `main.rs` and all integration suites are untouched by the
  move.
- `openapi.json` is asserted equal to the router's registered path set (`api::ROUTES` minus the
  `api::UNDOCUMENTED_ROUTES` aliases) and a test proves every declared route is actually served.
- **Payload change**: create and update validate the body against the resource's typed model before persisting,
  so a body carrying a field the resource does not define is rejected rather than stored. See *Breaking change
  accounting*.

## Error Contract and Schema Strictness Plan (Issue #76)

Issue: [#76](https://github.com/TucanoTechnology/TucanoTestAPI/issues/76) — `openapi.json` described an error
contract that the service does not return: the `400`s several operations actually answer were missing, the
`413` was published as the error envelope rather than the plain text the router sends, the multipart upload
route's extractor-level rejection was undocumented, and the published schemas were neither as strict as the
payload validation nor as permissive as the parsers that accept them. This plan reconciles the document with
the observed behaviour. It is documentation-only: no status code, response body, or validation rule changed.

### Validation order (the recorded decision)

Whichever check runs first decides the error a caller sees, so the order is part of the contract. The decision
for this issue was to **document where each code occurs**, not to re-order the checks:

1. **Body framing (extractor).** The `Json` and `Multipart` extractors run before the handler. A body they
   cannot frame is rejected there, and that rejection is plain text rather than the envelope (see
   *Extractor responses*).
2. **Payload validation.** `create` and `update` validate the body against the resource's typed model first:
   a non-object body, an unknown top-level key, or a field whose JSON type contradicts the model (a nested
   collection included) answers `400 invalid_request` and nothing is written. An empty object is a valid partial
   payload, so `PUT /test_cases/{id}` with `{}` proceeds to the identifier lookup rather than failing validation.
3. **Identifier resolution.** A path identifier that is not a single usable component answers
   `400 invalid_id`. Test cases are addressed verbatim, so an unusable case identifier is simply a case that
   does not exist (`404 not_found`) — never `invalid_id`.
4. **Parent resolution.** A write refuses a parent that does not exist with `404 not_found`, so a write can
   never invent a parent.
5. **Existence and ambiguity.** A missing document is `404 not_found`; an identifier several parents hold is
   `409 conflict`, with a message directing the caller to a parent-scoped route.

The composition routes read what they act on from the **body before** they consult the path parent, and that
ordering is observable. `POST /projects/{id}/test_suites` and `POST /projects/{id}/test_cases` read `name` /
`title` to choose between the create and place arms, and on the place arm they read the `suiteId` /
`testCaseId` they name; a body missing those fields answers `400 invalid_request` even when the path parent is
itself unusable. The unusable path identifier is reported as `invalid_id` through the create arm
(`require_parent`) or, on the place arm, once the named source has resolved. The run routes
(`POST /test_runs/{id}/test_suites`, `POST /test_runs/{id}/test_cases`, `POST /test_runs/{id}/results`)
likewise read the fields their body requires before they load the run. These routes therefore publish
`InvalidIdOrRequest` (and `/results`, whose body also carries a constrained `status`, publishes
`InvalidIdOrRequestOrStatus`). The whole order is pinned by
`tests/service.rs::an_unusable_path_identifier_is_answered_with_invalid_id`.

### Documented `400` codes

Every operation that can answer `400` now publishes one, using a named response component so the description
and code are visible in Swagger: `InvalidId` (`invalid_id`), `InvalidRequest` (`invalid_request`),
`InvalidIdOrRequest` (`invalid_id` or `invalid_request`), `InvalidIdOrRequestOrStatus` (those plus
`invalid_status`), and `AttachmentRejected` (`invalid_multipart` or `missing_file`). OpenAPI 3.0 ignores
siblings of `$ref`, so each distinct description is its own component even where the status code matches.

### Payload too large

The router caps every request body at `api::MAX_BODY_BYTES` (50 MiB, the attachment limit) with a
router-wide `RequestBodyLimitLayer`. Both the path that short-circuits on a declared `Content-Length` and the
path that streams a body through the limited reader answer `413` with `text/plain; charset=utf-8` and the body
`length limit exceeded` — the `BodyTooLarge` component, which deliberately publishes no JSON schema. Because
the layer runs before the handler, the per-handler `PayloadTooLarge` envelope is unreachable for every
published route; the guard remains as a backstop and is covered by
`tests/service.rs::an_oversized_body_is_rejected_in_plain_text_before_the_handler_runs`, which observes the
plain-text `413` on a JSON route, the upload route, and a run-results route. Every operation with a request
body documents the `413`.

### Extractor responses

`POST /test_cases/{id}/attachments` is the one route that can answer before its handler runs. A request the
multipart extractor cannot start on — a `Content-Type` without a boundary, for example — answers
`400 text/plain` and never the error envelope; once the framing parses, the handler's rejections use the
envelope (`missing_file` for a body with no file part). The route's `400` entry therefore publishes **both**
content types, which is why it is the `AttachmentRejected` component rather than plain `Error`. The accepted
`201` body the handler returns is documented as well. All three answers are asserted by
`tests/service.rs::the_upload_route_answers_plain_text_only_when_multipart_framing_is_unusable`.

### Schema strictness

`additionalProperties: false` is now published only where the API genuinely rejects unknown fields — the
typed models payload validation deserialises, all of which carry `deny_unknown_fields`: `Attachment`,
`Project`, `TestSuite`, `TestCase`, `TestStep`, `TestCaseResult`, `Milestone`, `TestConfiguration`. The
schemas stay permissive where the handler branches on a loose body or where the schema only ever describes a
response: `AttachmentUpload` (a multipart part), `CompositionRequest` (the create/place union), the duplicate
request bodies (`DuplicateRequest`, `DuplicateCaseRequest`, `DuplicateRunRequest`), `TestResultRequest`, and
the response-only `CompositionResponse`, `MilestoneProgress`, and `Error`. Publishing `additionalProperties:
false` on those would advertise a rejection the service does not perform — the opposite of the problem this
issue fixes. `tests/service.rs::openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields` holds
the split to the models.

`Error` itself carries no top-level `required`: the envelope's only required member is the nested
`error.code` / `error.message` pair.

### Corrections made

- `POST /test_runs/{id}/results` takes `TestResultRequest` (`testCaseId` and `status` required, `timestamp`
  and `notes` optional), not `TestCaseResult`. The result the run stores is a `TestCaseResult`, which also
  carries `attachments`; the request body cannot set those, so publishing the stored shape as the request
  advertised fields the route ignores.
- `TestCaseResult.timestamp` no longer claims a date `format`. It is whatever string the run body carried and
  is never parsed, so the document now says so.
- `Attachment` requires `filename`, `originalName`, `mimeType`, and `size`, with `uploadedAt` optional.

### Recorded, not changed

These were found while reconciling the document and are left as they are:

- A request whose upload content type is neither `multipart/form-data` nor a JSON body the extractor accepts
  can answer `415 Unsupported Media Type`, which the document does not publish.
- `TestCase.priority` and `TestCase.severity` are published with `enum` values while the model accepts any
  string, so the schema is narrower than the service. The values come from the legacy schema and are kept for
  parity; the widening (or the `enum` on the Rust side) needs its own decision.
- No `TestRun` schema is published, so the run documents referenced by responses are described only
  structurally. Adding one is a follow-up.

## Identity Normalisation Plan (Issue #78)

Issue: [#78](https://github.com/TucanoTechnology/TucanoTestAPI/issues/78) — a create body that omits the
identity field either stores a document that satisfies its model or is refused with a `4xx`, never `201`
followed by a `500`. Found while reconciling the error contract for #76, which recorded it as outside its scope;
this plan resolves it.

- Every resource with an identity field records it on write, from the identifier the create body already
  derived: `projectId`, `suiteId`, `testRunId`, `milestoneId`, `configId`. A value the body supplied is kept
  verbatim; the derived id only fills a field that is absent or not a string.
- A test run stored without a `timestamp` records the moment it was stored — Unix seconds rendered as a string,
  applied by the same rule (`required_string(body, "timestamp").unwrap_or_else(current_timestamp_string)`) the
  run-result route already used. A `timestamp` the body carries is kept.
- The milestone identity is the stored name, so `POST /milestones {"name": "M1"}` stores
  `milestoneId: "M1.json"`, the same id form duplication already derives.
- Test cases are unaffected: the id is derived from `testCaseId`, so a body that omits it is refused with
  `400 invalid_request` before anything is written. Storage stays permissive and reads are unchanged — no
  document already on disk is re-validated or rewritten.
- Parent markers are unchanged: `project.json` and `suite.json` still store empty `testSuites` / `testCases`
  arrays, because membership lives in the folders.
- Deviation recorded with tests in `tests/runs.rs::a_run_created_from_a_name_alone_reads_back_and_records_results`,
  `tests/milestones.rs::a_milestone_created_from_a_name_alone_reads_back_and_reports_progress`,
  `tests/configurations.rs::a_configuration_created_from_a_name_alone_reads_back_as_its_model`, and
  `tests/service.rs::an_unusable_path_identifier_is_answered_with_invalid_id`, which now creates its run from a
  name alone.

## Partial Update Plan (Issue #80)

Issue: [#80](https://github.com/TucanoTechnology/TucanoTestAPI/issues/80) — a `PUT` with a partial body used to
replace the stored document wholesale, so the fields the body left out were silently discarded: `PUT
/projects/P1.json {}` dropped `name` and `PUT /milestones/M1.json {}` stored `{}`, after which
`GET /milestones/M1.json/progress` answered `500 storage_error`. The ticket offered a merge and a refusal; this is
the recorded merge decision.

- **Decision: merge, not refuse.** A `PUT` overlays the top-level fields the body carries onto the document
  already stored; every other stored field keeps its value. Refusing a body that omits a required field of the
  target model — the other option the ticket offered — would have rejected the partial updates the API has always
  accepted (`{"name": "alpha"}`) and would have needed a required-field list per resource that validation does
  not have.
- **Only the top level is merged.** A field the body carries replaces the stored field as a whole, so an array
  (`testSuites`, `results`, `steps`) is replaced rather than concatenated, and nested objects are not
  deep-merged.
- **A field carrying `null` counts as not supplied** and keeps the stored value — the same reading of `null`
  `validate_payload` applies to every field. A `PUT` therefore cannot remove an optional field:
  removing one means deleting and recreating the document. This is what keeps a partial body from ever nulling a
  required field, so a `200` can no longer be followed by a `500` on a later read.
- **The rules that already applied to a write still apply after the merge.** The merged document is written
  through the same normalisation as creation, so a parent marker still stores empty `testSuites` / `testCases`
  arrays (Issue #65), the identity field is still recorded when it is absent or not a string (Issue #78), and a
  run still records a `timestamp` when it has none.
- **Bad input is still reported, never silently overwritten.** The body is validated before anything is read or
  written (unknown key → `400 invalid_request`), a missing document is still `404 not_found`, and an unusable
  identifier is still `400 invalid_id`. A stored document that is not a JSON object, or that is corrupt JSON, now
  answers `500 storage_error` with "Stored JSON is invalid" instead of being replaced with the body. The update
  path translates its read like every other read path rather than probing existence first, which also removes one
  filesystem round-trip and the race between the probe and the write.
- Deviation recorded with tests in `tests/projects.rs`, `tests/suites.rs`, `tests/cases.rs`, `tests/runs.rs`,
  `tests/milestones.rs` and `tests/configurations.rs` — each carries
  `a_partial_update_keeps_the_fields_the_body_leaves_out`, and the milestone suite also asserts
  `GET /milestones/{id}/progress` answers `200` after an empty `PUT`.

## Per-Step Attachment Plan (Issue #93)

Issue: [#93](https://github.com/TucanoTechnology/TucanoTestAPI/issues/93) — a structured step could describe an
action and an expected result but could not carry the evidence for it, so a screenshot or log belonging to one
step had to be attached to the whole case. This is the versioning plan for the schema field and the three
operations the fix adds, recorded before the implementation commits.

- **The field is additive and optional.** A step gains `attachments`, an array of
  `{filename, originalName, mimeType, size}`, and nothing else about `TestStep` changes. A response that carried
  no attachments before still carries none: the key is written only when the step owns at least one, so a case
  whose steps have no attachments serialises byte-for-byte as it did before the change. A client that reads a
  structured step without knowing the field is unaffected.
- **`TestStep` becomes strict about the new key, which is why this note is required.** The model carries
  `deny_unknown_fields`, so before the change a request body containing `attachments` on a step was rejected with
  `400 invalid_request` and now is accepted (and validated). This widens the accepted input rather than narrowing
  it — no payload that used to succeed is refused — and it is the only sense in which the stored-document
  contract changes. `TestCaseStep` (a step is either a plain string or this structure, `untagged`) is untouched,
  so a case whose steps are all plain strings is unchanged in every respect: it has no attachments, the new
  routes address it only to answer `400` saying the step is not structured, and its stored JSON is identical.
- **Simple string steps stay simple.** Attachments belong to structured steps only. A bare string step is not
  rewritten into an object to hold them, so the documented example cases and every case already on disk keep
  their shape.
- **Storage is a nested `steps/<index>/` directory inside the case folder**, beside the case-level attachment
  files. Step attachments and case attachments therefore cannot collide by name, `copy_dir_all` carries the
  directory recursively when a case is duplicated or placed, and the case marker's `steps[i].attachments` array
  is the index of what the directory holds — updated under the same storage lock as the file write, exactly as
  the case-level `attachments` array already is.
- **Three operations on two paths.** `POST /test_cases/{id}/steps/{step_index}/attachments` appends metadata and
  `201`s; `GET .../attachments` lists the metadata (`200`, an empty array when the step carries none); `DELETE
  .../attachments/{filename}` removes file and metadata and `404`s for a filename that is not attached. There is
  **no download route**: the ticket's endpoint list and its definition of done name upload, list and delete only,
  and the GUI flow that would consume a preview is paused. This is recorded as an intentional scope decision, so
  a client that needs the bytes reads the stored file through the documented filesystem layout rather than the
  API, and a download route would be a separate additive change.
- **The index is validated like the identifiers are.** `step_index` must be a non-negative integer, so a
  non-numeric or negative value is `400 invalid_request` — not `invalid_id`, which stays reserved for a path
  identifier, and not a new code, so the published set of error codes does not grow. An index that is an integer
  but addresses no step, and an index that addresses a plain string step, are also `400 invalid_request`, each
  naming the reason. A case addressed by an unusable identifier answers `404`, never `400`, matching the verbatim
  rule Issue #76 recorded for test cases.
- **`file_name` handling follows the case-level rules unchanged.** The uploaded part's filename passes
  `validate_component`, so `../escape`, a nested path, an empty name and `.` are refused; the stored name is
  prefixed with a unique suffix exactly as a case attachment is, so two uploads that share an original name do
  not overwrite each other; the file is written through the same atomic same-directory temp and `0o666`
  permissions; and no raw path or filesystem error reaches the client.
- **The error envelope is stable.** `400` with `invalid_request` or `invalid_multipart`, `404` with `not_found`
  (an absent case, an unattached filename), `409` when a bare case identifier is ambiguous, and `413` plain text
  for an oversized body are the responses `openapi.json` documents for these operations.
- Deviation recorded with tests in `tests/cases.rs` and `tests/attachments.rs` — upload/list/delete on a
  structured step, a simple step left untouched, an out-of-range index, a plain-string index, a non-numeric
  index, an unattached filename, and a traversing filename — plus repository unit tests over the physical
  `steps/<index>/` layout and `tests/service.rs` covering the documented routes and the strict `StepAttachment`
  schema.

## Tags Plan (Issue #49)

Issue: [#49](https://github.com/TucanoTechnology/TucanoTestAPI/issues/49) — projects, suites, cases and runs carry a
free-form `tags` array for labelling, filtering and release planning. The field, the filter and the document
entries are already on `main`; this note records the contract they stand for, so the behaviour the integration
suite pins is written down rather than inferred from it.

- **The field is additive and optional.** `Project`, `TestSuite`, `TestCase` and `TestRun` each gain
  `tags: Option<Vec<String>>`, so a document that carries none is unchanged: the key is skipped when absent, and
  neither create nor update ever writes an empty array in its place. A resource stored without tags reads back
  without the key, which is why an untagged document can never be matched by the filter.
- **Storage is verbatim.** Tags are stored exactly as supplied — case, order and duplicates preserved — and a
  partial update that names `tags` replaces the whole array while leaving the other keys alone, an explicit `[]`
  clearing it. The field is not normalised on write; only the comparison the filter performs is
  case-insensitive.
- **The array must be well-typed.** A `tags` value that is not an array of strings — a bare string, an object,
  or an array of numbers — is rejected with `400 invalid_request` naming `tags` (Issue #121). The model is the
  schema, so the verbatim storage above covers the strings a client supplies, not an arbitrary JSON value; the
  check is the general scalar type validation, not a rule specific to `tags`.
- **The filter is a shared comma-separated OR over the listed resource.** `?tags=a,b` keeps a resource when it
  carries *at least one* of the requested tags, compared case-insensitively with surrounding whitespace
  trimmed, and a resource without a `tags` array never matches. It composes with `?filter=` (substring over
  ids) and, on runs only, with `?configuration=`; the filters are applied in that order and all are
  conjunction.
- **Publishing the parameter is what this note corrects.** `?tags=` was published on all four top-level list
  operations, including `GET /milestones` and `GET /configurations`, whose models define no `tags` field and
  whose writes refuse one with `400 invalid_request` — so the documented filter could only ever answer an empty
  listing there. The parameter is now withheld from those two operations and stays on `GET /projects` and
  `GET /test_runs`, the list operations whose resource can actually store a tag. This is documentation only: the
  routes still accept and ignore the query parameter, and no response shape changed. `GET /test_suites` and
  `GET /test_cases` also filter by tags but are retired from the document as a whole
  (`api::UNDOCUMENTED_ROUTES`), as recorded under Issue #66.
- **Nested and parent-scoped listings take no query filter.** `GET /projects/{id}/test_suites`,
  `GET /projects/{id}/test_cases` and `GET /test_suites/{id}/test_cases` return the complete child set; only the
  four top-level list operations read `ListQuery`. A client that needs a filtered view of a project's suites
  filters the ids it receives.
- Deviation recorded with tests in `tests/tags.rs` — create/read/update round trips including an explicit
  empty array, case-insensitive and whitespace-trimmed matching, the any-of semantics, the untagged-never-matches
  rule, the filter on suites, cases and runs, the unknown-key rejection that keeps `deny_unknown_fields` intact,
  and the document assertion that `?tags=` is published only where a tag can be stored.

## JUnit XML Import Plan (Issue #85)

Issue: [#85](https://github.com/TucanoTechnology/TucanoTestAPI/issues/85) — a continuous-integration run produces
a JUnit XML report, and until now every `<testcase>` in it had to be typed into `POST /test_runs/{id}/results` by
hand. `POST /test_runs/{id}/import/junit` reads such a report and records the results it can name, reporting what
it did rather than failing on the parts it cannot use.

- **The body is the report, the content type carries no framing.** The route reads the raw request body (any
  `application/xml` document) and requires it to be UTF-8 and well-formed XML; a body that is neither answers
  `400 invalid_request` and writes nothing. There is no envelope around the XML and no multipart part.
- **Every `<testcase>` at any depth is a case.** The report is walked by descendant rather than by a fixed
  `testsuites`/`testsuite`/`testcase` path, so a nested suite or an unusual wrapper is still read. A testcase is
  named by `{classname}.{name}` when it carries a non-empty `classname`, else by `name` alone.
- **Status maps onto the existing run statuses.** A `failure` or `error` child is `Failed`, a `skipped` child is
  `Blocked`, and a testcase with none of those is `Passed`. No new status is introduced, so the recorded
  `TestCaseResult` is the same shape `POST /test_runs/{id}/results` writes. A failure's or error's `message`
  attribute becomes the result `notes`; a testcase's `timestamp` is the enclosing `testsuite`'s `timestamp`
  attribute when it has one, else the moment the import ran.
- **The report is a source of new results, never an overwrite.** A testcase the run already records — whether it
  was recorded before the import or repeated within the same report — is counted as a duplicate and left
  untouched, so a hand-recorded outcome is never replaced by a re-import. A `<testcase>` that cannot be named (no
  `name`) is counted as an error and skipped rather than failing the whole request, so a partly unusable report
  still imports the part that is usable.
- **The response is a summary, and it is the same envelope Issue #86 reuses.** `ImportSummary` carries
  `imported`, `skipped`, `errors`, `duplicates` and a nested per-status `summary`
  (`{passed, failed, blocked}`). `skipped` is `duplicates + errors`, and the per-status counts describe only the
  results actually written, so `passed + failed + blocked == imported`. A duplicate is not an error: `errors`
  counts only testcases that could not be named.
- **The route addresses a run like the result route does.** An unknown run is `404 not_found`, an unusable run
  identifier is `400 invalid_id`, and a body larger than the request limit is the router's plain-text `413`
  (Issue #76) — the import never runs after the fact. The run is loaded, its results overlaid, and the document
  written once, so a report that cannot be parsed or a run that cannot be found leaves the stored run exactly as
  it was.
- Deviation recorded with tests in `tests/runs.rs` — the three status mappings and the notes/timestamp they
  carry, duplicate and repeated cases left untouched, an absent suite timestamp filled from the clock, an
  unnamed testcase counted as an error, malformed and non-UTF-8 bodies rejected with nothing written, and an
  unknown run answered `404` — plus `src/domain/import.rs` unit tests over the parser itself, `tests/service.rs`
  covering the documented route and resolving its `ImportSummary` schema references, and the `roxmltree`
  dependency recorded in `docs/architecture/rust-service-core.md`.

## JSON Result Import Plan (Issue #86)

Issue: [#86](https://github.com/TucanoTechnology/TucanoTestAPI/issues/86) — Issue #85 reads a JUnit XML report, but
a runner without a JUnit reporter emits JSON. `POST /test_runs/{id}/import/json` reads an array of result entries
and records them against the run, reusing the `ImportSummary` envelope Issue #85 introduced.

- **The body is a bare array, or an object whose only field is `results`.** `[]` and `{"results": []}` both parse
  to no cases; a top-level object with any other key, or with a `results` that is not an array, is a
  `400 invalid_request`, so the wrapper cannot smuggle a second field past the importer.
- **An entry names a case and a status, and nothing else is assumed.** Each entry is an object with `testCaseId`
  and `status` required, and optional `notes` and `timestamp`. `testCaseId` must be non-empty and `timestamp` is
  stored verbatim, defaulting to the current time in Unix seconds — the same rule the single-result route and the
  JUnit importer apply. Unknown or misspelled fields are refused by `deny_unknown_fields` rather than silently
  dropped, because a misspelt `testCaseId` is the failure a caller most needs to hear about.
- **Only the three runner outcomes are accepted.** `status` must be `Passed`, `Failed` or `Blocked`. `Untested`
  and `Retest` are run-level states no runner report means, so they answer `400 invalid_status` (a distinct
  constructor from the five-status message the results route uses, which would name statuses an import never
  accepts). The per-status counts in the response therefore stay the three `ImportCounts` carries.
- **The import is strict and atomic, so `errors` is always zero.** Unlike the JUnit importer — which skips a
  testcase it cannot name and reports it as an error — a JSON body is either wholly usable or wholly refused: a
  malformed document, an unusable entry, or any bad field anywhere in the array answers `400` and writes nothing,
  because the entire body is parsed before the run is loaded. The run is then loaded, its results overlaid, and
  the document written once, so a failure at any point leaves the stored run exactly as it was. A case the run
  already records — including one repeated within the same body — is counted as a duplicate and left untouched.
- **The response is the shared summary.** `ImportSummary` carries `imported`, `skipped`, `errors`, `duplicates`
  and the nested per-status `summary`. `skipped` is `duplicates + errors`, and with a strict body `errors` is
  always 0, so a JSON import's `skipped` equals its `duplicates`.
- **The route addresses a run like the result and JUnit routes do.** An unknown run is `404 not_found`, an
  unusable run identifier is `400 invalid_id`, and a body larger than the request limit is the router's
  plain-text `413`.
- Deviation recorded with tests in `tests/runs.rs` — the field mapping and timestamp default, the `results`
  wrapper, duplicates left untouched, an entry missing or emptying a required field, an unknown field, a status
  outside the three, and a body that is not a readable array all rejected with nothing written, and an unknown
  run answered `404` — plus `src/domain/import.rs` unit tests over `parse_json` itself and `tests/service.rs`
  covering the documented route and its `ImportEntry` schema.

## Breaking change accounting

- **Tags added, and their query parameter withdrawn where it could not match** (Issue #49, plan above).
  `Project`, `TestSuite`, `TestCase` and `TestRun` gain an optional `tags` array and the list operations gain
  `?tags=a,b`. Additive only: no payload that used to succeed is refused, an existing document is never
  rewritten, and a response that carried no `tags` key still carries none. The one document change is the
  opposite direction — `?tags=` is removed from `GET /milestones` and `GET /configurations`, because neither
  resource can store a tag, so the parameter advertised a filter that always answered an empty listing. The
  routes still accept and ignore the parameter, so no request changes its answer. Deviation recorded with tests
  in `tests/tags.rs`, including
  `::the_tags_parameter_is_published_only_where_a_tag_can_be_stored`, which holds the document to the set of
  resources the models let carry tags.
- **Per-step attachments** (Issue #93, plan above). `TestStep` gains an optional `attachments` array and
  `/test_cases/{id}/steps/{step_index}/attachments` (GET, POST) and `.../attachments/{filename}` (DELETE) are
  added. The field is written only when a step owns attachments, so existing cases sign on disk and on the wire
  unchanged, and a new key inside a step that used to be refused becomes accepted. No download route is added.
  Deviation recorded with tests in `tests/cases.rs`, `tests/attachments.rs` and `tests/service.rs` as listed in
  the plan.
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
- **Retired flat creation routes** (Issue #66). `POST /test_suites` and `POST /test_cases` no longer create; they
  answer `400 Bad Request` naming the parent-scoped replacement. Both paths stay registered, so the published
  contract omits them as a whole — which also removes the global `GET /test_suites` and `GET /test_cases` scans
  from `openapi.json` even though the router serves them. The scans keep their distinct, sorted output and are
  covered by `tests/suites.rs` and `tests/cases.rs`; only their documentation is withdrawn. Deviation recorded
  with tests in `tests/service.rs::openapi_document_matches_the_registered_routes` (documented paths are exactly
  `api::ROUTES` minus `api::UNDOCUMENTED_ROUTES`) and `tests/suites.rs::the_flat_creation_route_only_explains_the_replacement`.
- **Create and update validate payloads before persisting** (Issue #71). A body carrying a top-level key the
  resource does not define is now rejected with `400 Bad Request` and the stable envelope
  (`{"error":{"code":"invalid_request","message":"Unknown field `x`"}}`), and nothing is written; previously any
  JSON object was stored verbatim, so documents carrying unknown keys could exist. Stored documents are never
  re-validated or rewritten, so existing data keeps working, and the partial payloads the API has always accepted
  (for example `{"name": "alpha"}`) still succeed. Composition, run-result, and duplicate endpoints keep their
  permissive parsing. Deviation recorded with tests in
  `tests/service.rs::create_and_update_reject_unknown_fields`.
- **`openapi.json` error contract reconciled** (Issue #76). Documentation-only: no status code, response body,
  or persisted document changed. The document **gains** the `400` responses the operations already answered,
  publishes the plain-text `413` (and the `413` on every operation with a request body), corrects the
  `/test_runs/{id}/results` request body to `TestResultRequest`, drops the `format` claim on
  `TestCaseResult.timestamp`, and adds `additionalProperties: false` to exactly the schemas whose payloads
  validation rejects unknown fields for. A consumer that generated a client from the old document may see new
  error responses it previously treated as undocumented; no previously published success response changed.
  Deviation recorded with tests in `tests/service.rs::openapi_documents_the_error_contract_of_every_operation`,
  `::an_oversized_body_is_rejected_in_plain_text_before_the_handler_runs`,
  `::the_upload_route_answers_plain_text_only_when_multipart_framing_is_unusable`,
  `::an_unusable_path_identifier_is_answered_with_invalid_id`,
  `::a_test_case_identifier_is_addressed_verbatim`,
  `::duplicate_routes_report_an_unusable_identifier_as_their_own_description_says`, and
  `::openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields`.
- **Identity fields recorded on write** (Issue #78). A create or update body that omits the identity field now
  stores the field the derived id stands for, so a name-only `POST /test_runs {"name": "nightly"}` stores more
  keys than before: `{"name": "nightly", "testRunId": "nightly.json", "timestamp": "…"}`. No request that used
  to succeed is refused, no published response shape changed, no document already on disk is rewritten, and a
  field the body supplied is still stored verbatim. The observable effect is that a route reading such a
  document — `GET /test_runs/{id}`, `GET /milestones/{id}`, `GET /milestones/{id}/progress`, `GET
  /configurations/{id}`, `POST /test_runs/{id}/{results,test_suites,test_cases}` — answers its document instead
  of `500 storage_error`. Deviation recorded with tests in
  `tests/runs.rs::a_run_created_from_a_name_alone_reads_back_and_records_results`,
  `tests/milestones.rs::a_milestone_created_from_a_name_alone_reads_back_and_reports_progress`,
  `tests/configurations.rs::a_configuration_created_from_a_name_alone_reads_back_as_its_model`, and
  `tests/service.rs::an_unusable_path_identifier_is_answered_with_invalid_id`.
- **Scalar fields are type-checked against the model** (Issue #121). `validate_payload` now type-checks every
  supplied top-level field, not only the nested collections it checked before: a body whose field carries the
  wrong JSON type is rejected with `400 Bad Request` and the stable envelope naming it
  (`{"error":{"code":"invalid_request","message":"Field `tags` is invalid"}}`), and nothing is written. The gap
  mattered most for `tags`, declared `Option<Vec<String>>` and published as
  `{"type":"array","items":{"type":"string"}}`: a document stored with `"tags": "smoke"` was served back as
  valid while the documented `GET /projects?tags=smoke` silently never matched it. The partial payloads the API
  has always accepted, a `null` field, and the unknown-key rejection are all unchanged, and stored documents are
  still never re-validated or rewritten, so a document persisted with an old wrong-typed field keeps answering
  `GET`. The check is driven by the models — a `Default` instance is serialised and each present field is probed
  against it — so a field renamed in `src/models.rs` cannot drift out of the check. Deviation recorded with
  tests in `tests/validation.rs` and `tests/tags.rs::a_wrong_typed_scalar_is_rejected_with_the_field_named`.
- **JUnit XML result import added** (Issue #85, plan above). `POST /test_runs/{id}/import/junit` and its
  `ImportSummary` / `ImportCounts` schemas are new; the route writes the same `TestCaseResult` the results route
  already does, so no stored document shape changed and no response that existed before was altered. Additive
  only: no request that used to succeed is refused, and a report can only add results a run did not already
  record. Deviation recorded with tests in `tests/runs.rs`, `src/domain/import.rs` and `tests/service.rs` as
  listed in the plan.
- **JSON result import added** (Issue #86, plan above). `POST /test_runs/{id}/import/json` and its `ImportEntry`
  schema are new; the route writes the same `TestCaseResult` the results route already does and reuses the
  `ImportSummary` / `ImportCounts` schemas Issue #85 introduced, so no stored document shape changed and no
  response that existed before was altered. Additive only: no request that used to succeed is refused, and an
  import can only add results a run did not already record. Deviation recorded with tests in `tests/runs.rs`,
  `src/domain/import.rs` and `tests/service.rs` as listed in the plan.

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
