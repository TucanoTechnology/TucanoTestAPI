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

**Amended in part by layout v3 (Issue #215).** This plan left runs, milestones and configurations in
root-level collections. [#215](https://github.com/TucanoTechnology/TucanoTestAPI/issues/215) moved all three
inside the project folder, and the tree below shows the current layout; the *Project-Scoped Runs, Milestones and
Configurations Plan (Issue #215)* section of this document and
[`docs/architecture/adr-storage-layout-v3.md`](../architecture/adr-storage-layout-v3.md) are the authority for
it. Everything else here is unchanged — markers, membership, identity, reads, attachments, security invariants
— with one consequence worth naming: the delete cascade now carries a project's runs, milestones and
configurations with it, so a run is no longer guaranteed to outlive the project it covered.

### v2 layout

```text
<data>/
  projects/
    <project>/
      project.json                     project metadata marker
      test_runs/<id>.json              point-in-time snapshot copies (v3, Issue #215)
      milestones/<id>.json             milestone documents (v3, Issue #215)
      configurations/<id>.json         environment configurations (v3, Issue #215)
      <test suite>/
        suite.json                     suite metadata marker
        <test case>/
          test-case.json               full case document (legacy shape)
          <attachment files>
      <test case>/                     case owned directly by the project
        test-case.json
        <attachment files>
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
  parents (the result of copy-on-include). Run, milestone and configuration ids are unique **within a project**
  as of layout v3 (Issue #215); before that they were globally unique because they were stored flat.
- Children of one parent share a single folder namespace: a suite base and a direct-case folder name cannot
  collide, and creating a second child with an existing name returns `409 Conflict`.
- Creating a child whose name equals a parent marker file name (`project.json` in a project, `suite.json` in a
  suite) collides with the marker file and returns `409 Conflict`.
- `test_runs`, `milestones` and `configurations` are reserved child names inside a project folder (layout v3):
  a suite or case folder may not take one of those names, and an attempt returns `409 Conflict`.

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
- Global dereference (`GET`/`PUT`/`DELETE /test_suites/{id}`, `/test_cases/{id}`, duplicate routes, and the
  **bare** attachment routes): zero occurrences → `404 Not Found`, exactly one → operate, two or more →
  `409 Conflict` with a message directing the caller to the parent-scoped endpoints. The parent-scoped
  attachment routes added by Issue #290 do not dereference globally at all — they name the holder — so a case
  id shared by two parents stays addressable through them. Lists never fail on duplicates; they de-duplicate.
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
  lives). Upload/download/delete act on the resolved occurrence; an ambiguous case id returns `409 Conflict`
  **on the bare routes** — see the parent-scoped mirrors below.
- The case marker's `attachments` array is updated under the same storage lock as the file operation so metadata
  and files never diverge for API-mediated changes.
- **Issue #290 — parent-scoped attachment routes (additive).** Every case- and step-attachment operation now
  also exists in two parent-scoped forms that name the folder holding the case:
  `/projects/{project_id}/test_cases/{case_id}/attachments[/{filename}]` and
  `/test_suites/{suite_id}/test_cases/{case_id}/attachments[/{filename}]`, plus the
  `/steps/{step_index}/attachments[/{filename}]` step forms. They address the named parent's occurrence
  directly and never resolve globally, so an attachment of a case id shared by two parents stays reachable
  instead of answering `409 Conflict`. No bare path, method, parameter or response shape changed, no field was
  added to a schema, and no pre-existing route was retired; the twelve new operations are purely additional
  surface. Step attachments still have no download route in any form. **Amended by Issue #289 (entry below):**
  the bare step download route now exists; the parent-scoped step forms still have none.
- **Issue #291 — downloads answer opaquely and name the file.** The three case-attachment downloads answer
  `Content-Type: application/octet-stream` whatever the stored file is — the media type the document already
  declared — and add `Content-Disposition: attachment` carrying an ASCII-safe `filename` plus, when the name is
  not plain ASCII, an RFC 5987 `filename*`. `Attachment.mimeType` keeps its meaning: it is the description of
  the stored file recorded in the case document, and it never becomes a response content type. The case upload
  route also records `uploadedAt` on the attachment it stores. **Amended by Issue #289 (entry below):** the
  step download is a fourth binary answer declaring the same media type and header, so `ContentDisposition` is
  now referenced from four `200`s.
- **Issue #289 — the bare step attachment downloads.** `GET /test_cases/{id}/steps/{step_index}/attachments/{filename}`
  is added, taking the documented count from 86 to 87. Where the case-level route has always answered bytes,
  this one previously answered `405 Method Not Allowed` with `allow: DELETE`, so the operation is purely
  additional: no bare path, method, parameter or response shape changed, no schema gained or lost a field, no
  stored document changed, and no request that used to succeed is refused. It mirrors the case download exactly,
  so the body is always `application/octet-stream` — the media type the document already declared — plus
  `Content-Disposition: attachment` naming the uploaded file (Issue #291 behaviour above). The stored
  `StepAttachment.mimeType` stays the description recorded in the document and never becomes a response content
  type; the entry keeps the narrow `StepAttachment` shape and gains no `uploadedAt`, because that key belongs
  only to the case-level `Attachment` recorded by Issue #291 and adding it to a step attachment would be a
  schema change no issue asks for. An unattached filename answers `404`, and so does a `{step_index}` that names
  no structured step — the byte route reads the file the index constructs rather than validating the index
  against the case, so a `{step_index}` that is not a non-negative integer is the only bad-request case
  (`400 invalid_request`), while an index that simply names no step is not found. The parent-scoped step
  families added by Issue #290 still have no download form in either direction, which is the one remaining
  asymmetry in the attachment surface.

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
request bodies (`DuplicateRequest`, `DuplicateCaseRequest`, `DuplicateRunRequest`), and the response-only
`CompositionResponse`, `MilestoneProgress`, and `Error`. Publishing `additionalProperties:
false` on those would advertise a rejection the service does not perform — the opposite of the problem this
issue fixes. `tests/service.rs::openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields` holds
the split to the models.

**Amended by the run-result plan (Issues #284/#285, below):** `TestResultRequest` moved from the permissive
list to the strict one, because the results route now rejects an unknown field in its body instead of
dropping it. `tests/service.rs` holds it in the strict group accordingly.

`Error` itself carries no top-level `required`: the envelope's only required member is the nested
`error.code` / `error.message` pair.

### Corrections made

- `POST /test_runs/{id}/results` takes `TestResultRequest` (`testCaseId` and `status` required, `timestamp`
  and `notes` optional), not `TestCaseResult`. The result the run stores is a `TestCaseResult`, which also
  carries `attachments`; the request body cannot set those, so publishing the stored shape as the request
  advertised fields the route ignores.
- `TestCaseResult.timestamp` no longer claims a date `format`. It is whatever string the run body carried and
  is never parsed, so the document now says so.
- `Attachment` requires `filename`, `originalName`, `mimeType`, and `size`, with `uploadedAt` optional. The
  field stays optional because documents written before Issue #291 carry no such key; the case upload route
  now records it (ISO-8601 UTC) so a new attachment reports when the API stored it.

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
  verbatim on create for a suite, a run and a milestone; the derived id only fills a field that is absent or not
  a string. The configuration identity is the one exception, and only since Issue #288: a `configId` is resolved
  to the file that holds it, so a stored value that disagreed with its own document would name nothing, and
  `configId` is therefore taken from the name on every write whether the body supplied one or not. Issue #300
  settles the remaining two readings of this rule — a `projectId` on create names the new project or is refused,
  and on update the identity field is checked rather than obeyed.
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
  identifier is still `400 invalid_id`. An identifier the update body carries is checked before the merge rather
  than part of it, which Issue #300 settled — see its entry under breaking change accounting. A stored document
  that is not a JSON object, or that is corrupt JSON, now
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
  API, and a download route would be a separate additive change. **Amended by Issue #289 (entry below):** that
  separate additive change arrived, in the bare form only — see its entry under breaking change accounting.
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
  (`api::UNDOCUMENTED_ROUTES`), as recorded under Issue #66. **Amended by Issue #293 (entry below):** the
  parameter is now also published on the three project-scoped listings, which accept and honour it —
  `GET /projects/{id}/test_suites`, `GET /projects/{id}/test_cases` and `GET /projects/{id}/test_runs`. The
  `GET /test_runs` named above is the retired flat route, no longer in the document since Issue #215; the run
  listing that carries the parameter is `GET /projects/{id}/test_runs`.
- **Nested and parent-scoped listings take no query filter.** `GET /projects/{id}/test_suites`,
  `GET /projects/{id}/test_cases` and `GET /test_suites/{id}/test_cases` return the complete child set; only the
  four top-level list operations read `ListQuery`. A client that needs a filtered view of a project's suites
  filters the ids it receives. **Amended by Issue #293 (entry below):** the three *project-scoped* lists
  `GET /projects/{id}/test_suites`, `GET /projects/{id}/test_cases` and `GET /projects/{id}/test_runs` now read
  `ListQuery` and honour `?filter=`, `?tags=` and — on the run listing — `?configuration=`.
  `GET /test_suites/{id}/test_cases` keeps the exhaustive behaviour this bullet describes.
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

## Defect Link Plan (Issue #87)

Issue: [#87](https://github.com/TucanoTechnology/TucanoTestAPI/issues/87) — a failed result needs to name the defect
it raised, and the first read of that link is a listing. This issue adds the `DefectLink` shape and
`GET /test_runs/{id}/results/{case_id}/defects`; creating and removing links is Issue #88.

- **The link is a document, not a bare URL.** `DefectLink` carries `linkId`, `defectId`, `defectUrl`,
  `trackerType` and `linkedAt` as required fields, with optional `title` and `status`. `trackerType` is one of
  `jira`, `github`, `gitlab` or `custom`, and `linkedAt` is stored verbatim (this API renders the current time as
  Unix seconds in a string, but a client-supplied value is kept as given). `linkId` is the link's own identity so
  the same defect can be linked, unlinked and relinked without the list position mattering — which is what makes
  the Issue #88 routes addressable.
- **Links live on the result, inside the run.** `TestCaseResult` gains an optional `defectLinks` array, so a
  link travels with the run snapshot that recorded it, and a run persisted before this change still reads
  (`defectLinks` is absent, not empty). The field is omitted from the wire when a result links nothing, the same
  way `attachments` and the other optional fields are.
- **The listing answers an empty list and a missing result differently.** A result that exists and links nothing
  answers `200 {"defects": []}`; a run that does not exist, or one that records no result for `case_id`, answers
  `404 not_found`, because "nothing is linked" and "there is nothing to link to" are different answers. An
  unusable run identifier answers `400 invalid_id` before storage is consulted, the rule every run-scoped route
  follows.
- **The response is an envelope.** `{"defects": [...]}` rather than a bare array, so a later issue can add
  summary fields beside the list without changing the response's shape. This is the first list route to use a
  named key; the other collection reads keep their bare arrays.
- Deviation recorded with tests in `tests/runs.rs` — an empty list for a result that links nothing, the links a
  stored result carries read back in order, and the `400`/`404` answers for an unusable identifier, an unknown
  run and a case the run never ran — plus `tests/service.rs` holding the `DefectLink` schema to
  `additionalProperties: false` and the route to the published contract, and model unit tests in
  `src/models.rs` for the camelCase wire shape and the omission of `title`/`status`.

## Defect Link Writes Plan (Issue #88)

Issue: [#88](https://github.com/TucanoTechnology/TucanoTestAPI/issues/88) — Issue #87 gave a failed result a
way to *read* the defects it raised; this issue gives it a way to name and unname one, through
`POST /test_runs/{id}/results/{case_id}/defects` and
`DELETE /test_runs/{id}/results/{case_id}/defects/{link_id}`.

- **The client supplies half the link and the API derives the rest.** The request body is a new
  `DefectLinkRequest` — `defectId`, `defectUrl` and `trackerType` required, `title` and `status` optional. It is
  deliberately *not* `DefectLink`: `linkId` and `linkedAt` are the API's to mint, so a body that carries either
  is rejected as an unknown field rather than silently overriding what the server decided. `linkId` is derived
  from the storage layer's monotonic `unique_suffix()` and `linkedAt` from the current Unix seconds, exactly as
  the other create routes stamp their own fields.
- **A link is created, not replaced, so the answer is `201` with the derived identity.** The response is
  `{"message": "Defect linked to test result", "id": "<linkId>"}`. The id has to come back: `DELETE` addresses the
  link by it, and the client has no other way to learn a value the server minted. This is the same shape the
  other create routes answer with.
- **`defectUrl` is validated against `trackerType` before anything is written.** `jira` requires a
  `<org>.atlassian.net/browse/<KEY>` URL, `github` a `github.com/<owner>/<repo>/issues/<number>` URL and `gitlab`
  a `gitlab.com/<group>/<project>/-/issues/<number>` URL; `custom` accepts any well-formed `https://` URL with a
  non-empty host, because there is no third-party shape to hold it to. A mismatch, a non-`https` scheme, a
  malformed host and an empty required field all answer `400 invalid_request`. The check is hand-rolled: this
  crate carries no URL parser, and the accepted grammar per tracker is narrower than a general URL would allow
  anyway. A query string or fragment is ignored, since a browser copy-paste carries one.
- **A defect can be linked to a result once.** Linking a `defectId` that the result already carries answers
  `409 conflict` and the original link survives untouched; a different `defectId` is a new link. The identity is
  per result — the same defect on another `case_id` is a separate link, because two results can legitimately
  raise the same bug.
- **The unlink route is `DELETE` and it answers `200` with a message.** `DELETE` on an unknown `link_id`, or one
  that belongs to another result, answers `404 not_found`; repeating a successful unlink is therefore a `404`,
  not an idempotent `200`. The link is gone from the result's `defectLinks` afterwards.
- **`link_id` is opaque and is never validated as a document name.** Unlike `{id}`, which names a stored run and
  answers `400 invalid_id` when it cannot, `{link_id}` is an identifier the API minted inside a document — it is
  looked up in the result's own list. It is published as its own `link_id` path parameter rather than reusing
  `id`, so the contract does not imply a validation that is not performed.
- **Failure answers follow the run-scoped rule.** An unusable run identifier answers `400 invalid_id` before
  storage is consulted; an unknown run, a `case_id` the run never ran, and an unknown `link_id` answer
  `404 not_found`. The body is validated before the run is loaded, so a malformed request never touches storage.
- Deviation recorded with tests in `tests/runs.rs` — all four tracker types linked and read back, twelve
  rejected bodies (missing, empty and mistyped fields, a body carrying `linkId`/`linkedAt`, an unknown tracker,
  a per-tracker URL mismatch and a non-`https` URL) leaving the result untouched, the duplicate `409`, and the
  `400`/`404` answers for an unusable `{id}`, an unknown run, an unrun `case_id`, a repeated unlink and an
  unknown `link_id` — plus `tests/service.rs` holding `DefectLinkRequest` to `additionalProperties: false` and
  `openapi.json` documenting both routes, the `link_id` parameter and the new schema.

## Test Case Versioning Plan (Issue #90)

Issue: [#90](https://github.com/TucanoTechnology/TucanoTestAPI/issues/90) — persists the version numbers and
revision snapshots the normative plan in
[`test-case-versioning-plan.md`](./test-case-versioning-plan.md) describes. This issue is storage only: the read
routes that expose the history belong to [#91](https://github.com/TucanoTechnology/TucanoTestAPI/issues/91) and
the run-side capture to [#92](https://github.com/TucanoTechnology/TucanoTestAPI/issues/92).

- **`TestCase` gains two optional, API-managed fields.** `version` is an integer and `lastModified` is a
  `date-time` string. Both are response-only: a create stamps `version: 1` and the current instant, an update
  rewrites them itself, and a value the body supplies is ignored rather than honoured. Neither is required, so
  a document written before this issue still deserializes and still reads back.
- **Only a qualifying change starts a version.** The plan names `title`, `steps`, `preconditions` and
  `expectedResult` as the fields that carry the case's substance, so an update that changes any of them
  snapshots the *previous* live document to `<case>/revisions/v{version}.json` and then increments `version`,
  writing a fresh `lastModified`. Any other update — a `priority`, `description`, `severity`, `testType`,
  `exploratory` or `tags` edit — leaves both `version` and `lastModified` exactly as they were.
- **A snapshot is immutable.** The snapshot file is named for the version it holds, so writing it twice is not
  meaningful: `save_revision` returns early when the marker already exists, and history stays append-only.
- **`lastModified` is ISO-8601 UTC, which diverges from the crate's other timestamps.** This API's older
  timestamps (`TestRun.timestamp`, `TestCaseResult.timestamp`, `DefectLink.linkedAt`) are Unix seconds rendered
  as a string, because their routes store a client's value verbatim. `lastModified` is different: it is never
  client-supplied, so it is written in the format the plan asks for — `YYYY-MM-DDTHH:MM:SSZ`. The formatter is
  hand-rolled from the Unix clock (`current_iso8601_timestamp`), because this crate carries no date library.
- **A document that predates versioning keeps its shape until it qualifies.** A case stored without `version`
  or `lastModified` reads back without them, and a non-qualifying update still leaves them absent — the fields
  are only added when the case is created or a qualifying change bumps it, so no old document is rewritten just
  by being read or lightly edited.
- **A copy carries its history with it.** Composition `copy` duplicates the case folder, so the new case already
  holds the source's `revisions/` snapshots and starts its own history from the source's current version.
  `duplicate` keeps the source document verbatim, so the copy shares that state — the plan's "a copied case
  starts its own history carrying the source's snapshots".
- Deviation recorded with tests in `tests/cases.rs` — a create stamping `version: 1`, ignoring a client's
  `version`/`lastModified` and creating no `revisions/` folder; a qualifying update writing an immutable
  `revisions/v1.json` holding the pre-update document and advancing to `version: 2`; a second qualifying update
  writing `v2.json` without rewriting `v1.json`; a non-qualifying update leaving the version, the stamp and the
  absence of a snapshot untouched; a pre-versioning document reading back without the fields and only gaining
  them on a qualifying edit; and a copied case arriving with the source's snapshot — plus `tests/service.rs`
  holding the `TestCase` additions to the published contract and `src/domain/mod.rs`, `src/models.rs` and
  `src/storage/layout.rs` carrying the formatter, the fields and the `revisions/` path as unit tests.

## Run Case-Version Capture Plan (Issue #92)

Issue: [#92](https://github.com/TucanoTechnology/TucanoTestAPI/issues/92) — the run-side capture the test-case
versioning plan leaves to a sibling of [#52](https://github.com/TucanoTechnology/TucanoTestAPI/issues/52). It
makes a run record the revision of each case it snapshotted, so editing a case never changes what a past run
reports.

- **`TestRun` gains one optional field.** `caseVersions` is a JSON object keyed by the case's `testCaseId` and
  valued with the revision the run pinned for it. The Rust field is
  `case_versions: Option<HashMap<String, u64>>` with `skip_serializing_if`, so a run that pinned nothing — every
  run written before this issue — keeps its exact stored shape and still deserializes.
- **The capture is first-write-wins.** `add_case_to_run` and `record_run_result` both call one helper
  (`composition::capture_case_version`) that inserts with `or_insert`, so re-recording a result, or a later
  qualifying edit to the live case, never revises a version the run already pinned: the run is a snapshot, and
  its `caseVersions` has to stay one too.
- **A case with no version of its own is pinned as `1`.** A case written before
  [#90](https://github.com/TucanoTechnology/TucanoTestAPI/issues/90) carries no `version`; the capture stores
  `1` for it, matching how the read routes treat such a case.
- **A result may name a case the store does not hold.** The results route has always accepted a `testCaseId` no
  case document matches, and it still does: the case is looked up best-effort, pinned at its own version when it
  can be read and at `1` when it cannot.
- **The field is client-editable through the run's own write routes.** The route capture is what *the API*
  writes on the two capture routes; a whole-document `PUT` that supplies `caseVersions` is accepted and stored,
  exactly as the run's other client-editable collections (`results`, `configurations`) are. The alternative —
  refusing the key — would break the read-modify-write `PUT` a client uses to round-trip a run it just read.
- **The import paths do not capture versions.** #92 scopes the capture to `add_case_to_run` and
  `record_run_result`, so a JUnit or JSON import still writes results without pinning versions.
- Deviation recorded with tests in `tests/runs.rs` (`::a_run_pins_the_case_version_it_snapshotted` and
  `::recording_a_result_pins_the_version_of_the_case_the_store_holds`), with the `TestRun` round-trip and
  absent-field unit tests in `src/models.rs` and `tests/service.rs` holding the `caseVersions` addition to the
  published contract.

## Coverage Report Plan (Issue #94)

Issue: [#94](https://github.com/TucanoTechnology/TucanoTestAPI/issues/94) — the first child of
[#37](https://github.com/TucanoTechnology/TucanoTestAPI/issues/37), whose scope says to "report test case count per
project section/suite". This child introduces the shared reports module the summary-report sibling builds on.

- **One read-only route.** `GET /reports/coverage` answers the counts as
  `{"projectId"?, "totalCases", "suites": [{"suiteId", "name", "caseCount"}]}`, published under
  `CoverageReport` and `SuiteCoverage` in `openapi.json`. Nothing is written, so no stored document changes.
- **`projectId` restricts the report; omitting it widens the scope to every project.** The parameter takes the
  same `<folder>.json` wire id every other project-addressed route takes, and the response echoes it back only
  when it was supplied. An unknown project is `404 not_found`; an identifier that cannot address a project at
  all (no `.json`) is `400 invalid_id`, exactly as `require_parent` answers elsewhere.
- **`totalCases` counts a project's own cases as well as its suites'.** The storage layout lets a case live
  directly under a project (`projects/<project>/<case>/test-case.json`) as well as inside a suite, and the
  repository lists both. The issue text says "per project section/suite"; there is no "section" resource in the
  tree, so a project's directly-held cases are that analogue. The report therefore sums them into `totalCases`
  while listing only suites, which means `totalCases` **can exceed** the sum of the `caseCount` values. This is
  recorded here because a client that adds up `caseCount` will otherwise read the totals as inconsistent.
- **The models follow the response-model convention.** The issue text asks for `deny_unknown_fields` on the new
  models, but `src/models.rs` applies that only to writable request models; response models (`MilestoneProgress`,
  `CaseHistoryEntry`) use bare `#[serde(rename_all = "camelCase")]`. `CoverageReport` and `SuiteCoverage` are
  response-only, so they follow the existing convention and are permissive. The deviation is deliberate and is
  the only one from the issue text.
- **A suite's `name` comes from its marker document.** The walk reads each suite's `suite.json` (`name`) and
  falls back to the identifier when a legacy document omits the field, so the entry is never nameless.
- Deviation recorded with tests in `tests/reports.rs`
  (`::an_empty_tree_reports_no_cases`, `::a_scoped_report_counts_the_suites_and_the_projects_own_cases`,
  `::a_global_report_sums_every_project`, `::a_project_that_does_not_exist_is_not_found`,
  `::an_unusable_project_identifier_is_invalid_id`), with the aggregation unit tests in `src/domain/reports.rs`
  (`::an_empty_scope_reports_no_cases_and_no_suites`, `::a_scoped_report_echoes_the_identifier`,
  `::cases_held_directly_by_a_project_join_the_total`,
  `::a_global_report_sums_every_project_and_keeps_their_suites`), and with
  `tests/service.rs::openapi_document_matches_the_registered_routes` holding the path to the registered route.

### Milestone progress: the buckets partition `totalCases` (Issues #195, #286)

`GET /milestones/{id}/progress` reports `totalCases` alongside five status buckets (`passed`, `failed`,
`blocked`, `untested`, `retest`). [#195](https://github.com/TucanoTechnology/TucanoTestAPI/issues/195) froze
the legacy arithmetic, in which the two counted different populations and a client that added up the buckets
could read the payload as inconsistent. [#286](https://github.com/TucanoTechnology/TucanoTestAPI/issues/286)
replaced that arithmetic with a single population, so the buckets now **partition** `totalCases`:

- **One population per linked run, deduplicated by case id.** A run holds the cases its `testCases` snapshot
  declares, the cases its embedded `testSuites` declare, and the cases it has a recorded result for; the
  population is their union, and a case that appears in more than one of those places counts once, with its
  recorded status when it has one.
- **The five buckets partition the population.** `passed + failed + blocked + untested + retest` always
  equals `totalCases`, so a client that adds the buckets up can never see them disagree. A held case with no
  recorded result — and any case stored under a status the API does not recognise — is `untested`; `Retest`
  keeps its own bucket.
- **The `totalCases == 0` fallback is gone.** It existed only to reconcile the two populations, which no
  longer exist. A milestone that links no runs, or whose linked runs hold no cases, reports `0` for every
  counter.
- **`passPercentage` divides by `totalCases`.** It is `passed / totalCases * 100`, unrounded, matching the
  exact-fraction figure the summary report publishes (`33.33333333333333` rather than `33.3`), and is `0` when
  `totalCases` is `0`. Because the buckets partition the total, the percentage is always inside `0..=100`;
  the #195 arithmetic could report above `100` when results outnumbered the declared snapshot.
- **This is a deliberate break from #195, and the only one from the legacy handler.** `compute` was
  byte-for-byte the pre-layering handler (`git show a1bf7ba^:src/api.rs` lines 677-735 carried the same
  `total_cases += cases.len()` and the `if total_cases == 0 { … }` fallback). The counts a milestone reports
  for a run that records results beyond its snapshot therefore change; the field names, the response shape
  and the `MilestoneProgress` schema do not.
- Covered by `tests/milestones.rs` (`::milestone_progress_aggregates_linked_test_runs`,
  `::progress_counts_the_cases_a_linked_suite_embeds`,
  `::progress_counts_recorded_cases_the_snapshot_never_declared`,
  `::a_milestone_created_from_a_name_alone_reads_back_and_reports_progress`), by
  `src/domain/progress.rs` (`::a_run_contributes_every_case_it_holds`,
  `::cases_embedded_in_a_linked_suite_are_part_of_the_population`,
  `::results_beyond_the_declared_snapshot_extend_the_population`,
  `::every_shape_of_run_keeps_the_buckets_summing_to_the_total`), and by
  `src/domain/service/tests.rs::milestone_progress_counts_every_case_a_run_holds_once`.

## Summary Report Plan (Issue #95)

Issue: [#95](https://github.com/TucanoTechnology/TucanoTestAPI/issues/95) — the second child of
[#37](https://github.com/TucanoTechnology/TucanoTestAPI/issues/37), building on the shared `src/domain/reports.rs`
module the coverage sibling introduced.

- **One read-only route.** `GET /reports/summary` answers
  `{"total", "passed", "failed", "blocked", "untested", "passPercentage", "totalDurationMs"}`, published under
  `SummaryReport` in `openapi.json`. Nothing is written, so no stored document changes.
- **The status mapping matches `MilestoneProgress`.** `Passed`, `Failed`, `Blocked` and `Untested` each count
  into their own bucket. `Retest` — and any status the API does not recognise — counts toward `total`, and so
  toward `passPercentage`'s denominator, but into no bucket. `passPercentage` is `passed / total * 100`, or `0.0`
  when nothing is in scope, and is **not** rounded: the issue's illustrative `79.2` is the rounded form of
  `95 / 120 * 100`, and the endpoint answers the exact fraction, matching how `MilestoneProgress` already reports
  `33.33333333333333` rather than `33.3`.
- **Every filter is optional and they intersect.** `projectId`, `milestoneId`, `configurationId`, `from` and `to`
  may be supplied together, and a run contributes only when it satisfies all of them. With none supplied every
  recorded result across every run is summarised.
- **`from` and `to` bound a run's own `timestamp`, inclusively.** Both accept a bare `YYYY-MM-DD` date or a full
  ISO-8601 timestamp, normalised to the date. A run stores its timestamp either as Unix seconds rendered as a
  string (the shape the API writes by default) or verbatim as the ISO-8601 value the client supplied; both reduce
  to a date. A run whose timestamp is in neither shape has no comparable date and is **left out** of a
  date-filtered report rather than silently counted. A value that is not a date in either shape is
  `400 invalid_request`.
- **`totalDurationMs` sums `durationMs` over the results in scope.** The field is optional and new on both the
  recorded result and the body that records one; a result without it contributes nothing. This is the only
  stored-document change, and it is additive because the field is written only when a client supplies one.
- **`total` counts every result, so it can exceed the four named buckets.** A report for a run that recorded a
  `Retest` has a larger `total` than the buckets sum to. This is recorded because a client that adds up the
  buckets would otherwise read the payload as inconsistent.
- **Unknown filter ids are `404`; unusable ones are `400`.** An unknown `projectId` is `404 not_found`
  (via `require_parent`); an unknown `milestoneId` is `404 not_found` with the milestone-specific message; an
  unknown `configurationId` is `404 not_found` with the generic `Resource not found` message the rest of the API
  uses for a missing resource. An identifier that cannot address the resource at all (no `.json`) is
  `400 invalid_id`.
- **The model follows the response-model convention.** The issue text asks for `deny_unknown_fields`, but
  `src/models.rs` applies that only to writable request models; `SummaryReport` is response-only, so it follows
  `MilestoneProgress`, `CaseHistoryEntry` and `CoverageReport` and uses bare `#[serde(rename_all = "camelCase")]`.
  As with the coverage sibling this is the only deliberate deviation from the issue text.
- Deviation recorded with tests in `tests/reports.rs`
  (`::an_empty_tree_reports_an_all_zero_summary`, `::a_summary_buckets_every_status_and_sums_the_durations`,
  `::results_from_every_run_join_an_unfiltered_summary`,
  `::the_filters_combine_and_each_restricts_the_runs_that_contribute`,
  `::a_milestone_filter_keeps_only_the_runs_it_references`,
  `::the_date_bounds_are_inclusive_and_leave_out_uncomparable_runs`,
  `::an_unknown_project_or_milestone_is_not_found`, `::an_unknown_configuration_is_not_found`,
  `::an_unusable_filter_identifier_is_invalid_id`, `::an_unusable_date_filter_is_invalid_request`), with the
  aggregation unit tests in `src/domain/reports.rs` (`::an_empty_result_set_reports_zeroes`,
  `::the_buckets_split_by_status_and_retest_counts_only_toward_the_total`,
  `::the_issue_example_produces_the_documented_pass_rate`,
  `::durations_sum_and_a_result_without_one_counts_as_zero`,
  `::a_project_filter_keeps_only_runs_that_embed_it`,
  `::a_milestone_filter_keeps_only_the_runs_it_references`,
  `::a_configuration_filter_keeps_only_linked_runs`, `::date_bounds_are_inclusive`,
  `::a_run_without_a_comparable_date_is_left_out_when_a_date_filter_is_set`,
  `::a_date_filter_accepts_a_bare_date_or_a_timestamp`), and with
  `tests/service.rs::openapi_document_matches_the_registered_routes` holding the path to the registered route.

## Request ID Plan (Issue #106)

Issue: [#106](https://github.com/TucanoTechnology/TucanoTestAPI/issues/106) — the request-id child of
[#14](https://github.com/TucanoTechnology/TucanoTestAPI/issues/14), the observability parent.

- **One middleware at the router boundary.** `src/api/request_id.rs` resolves an id before the request reaches
  any route, publishes it for the rest of the request, and echoes it on the response. Nothing is written, so no
  stored document changes.
- **An inbound `X-Request-Id` is honoured when it is usable.** The header is accepted verbatim — including the
  exact bytes, so the response echoes what the client sent — unless it is absent, empty, or not visible ASCII,
  in which case one is minted. Client input is never trusted into a panic.
- **A minted id is 16 hexadecimal characters.** It combines a process-local [`RandomState`] and an `AtomicU64`
  counter, so two requests differ without adding a dependency. It is **not** cryptographic: the id is a
  correlation handle, not a secret, and the plan deliberately does not call it one.
- **The id is published in a task-local scope, not an `Extension` alone.** Both exist: the `Extension` lets the
  span maker read it, and the task-local scope lets the error envelope read it without threading it through
  every handler.
- **Every answer carries the response header.** The middleware is the outermost layer, so the plain-text
  rejections the multipart extractor and the body-limit layer produce carry `X-Request-Id` too, not only the
  answers that pass through a handler.
- **The error envelope gains an optional `requestId`.** `ErrorBody` serialises it with
  `skip_serializing_if = "Option::is_none"`, so a caller outside a request scope — and the enum's own unit test —
  still sees the historical `{"error":{"code":…,"message":…}}`. `Error.properties.error` documents the field but
  does not add it to `required`, and `Error` stays free of `additionalProperties` to satisfy
  `tests/service.rs::openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields`.
- **The id is written to the request span.** `TraceLayer::make_span_with` is replaced with a `http.request` span
  carrying `method`, `uri` and `request_id`. No subscriber is installed here: #15 owns logging configuration, so
  the span exists and is populated but nothing renders it by default.
- **The contract declares the header on every response.** `openapi.json` gains
  `components.headers.XRequestId` and a `X-Request-Id` reference on **all** response objects — the eleven shared
  `components.responses.*` members and the inline answers alike — because OpenAPI forbids a `$ref` response from
  carrying sibling keys, so a header cannot be added at the reference site.
- **`tracing` is added without growing the lockfile.** `Cargo.toml` pins
  `tracing = { version = "0.1", default-features = false, features = ["std"] }`: the crate was already in
  `Cargo.lock` transitively through `tower-http`, and trimming the default `attributes` feature keeps the second
  `syn` it would otherwise pull out of the graph, so `audit`, `sbom` and `container-scan` see no new crate. The
  deliberate omission of `tracing-subscriber` is the same call — it belongs to #15.
- Deviation recorded with tests in `tests/request_id.rs`
  (`::a_request_without_the_header_gets_one`, `::a_request_with_the_header_echoes_it`,
  `::an_empty_header_is_replaced`, `::two_requests_get_different_ids`,
  `::an_error_envelope_carries_the_request_id`, `::a_plain_text_rejection_carries_the_header`,
  `::the_span_carries_the_request_id`), with the unit tests in `src/api/request_id.rs`
  (`::minted_ids_differ_and_are_sixteen_hex_characters`, `::current_is_absent_outside_a_request`), and with
  `src/api/error.rs::the_envelope_carries_a_code_and_a_message` holding the out-of-band shape unchanged.

## OpenAPI Typing Plan (Issue #140)

Issue: [#140](https://github.com/TucanoTechnology/TucanoTestAPI/issues/140) — the document published the ten
operations that accept a JSON body with a bare `{"type": "object"}` request schema, published 31 of the `2xx`
answers as a description with no body at all, and left `Error.code` an unconstrained string, so a generated
client could not tell what a write accepts from the document alone. This plan types all three. It is
documentation-only: no status code, response body, validation rule or stored document changed, and the
behaviour the document now describes was already pinned by the tests cited below.

### Write request bodies (G3)

The ten operations that read a JSON body now `$ref` a named schema instead of the open object:

- `POST /projects` → `ProjectCreateRequest`; `PUT /projects/{id}` → `ProjectUpdateRequest`.
- `PUT /test_suites/{id}` → `TestSuiteUpdateRequest`.
- `POST /test_runs` → `TestRunCreateRequest`; `PUT /test_runs/{id}` → `TestRunUpdateRequest`.
- `PUT /test_cases/{id}` → `TestCaseUpdateRequest`.
- `POST /milestones` → `MilestoneCreateRequest`; `PUT /milestones/{id}` → `MilestoneUpdateRequest`.
- `POST /configurations` → `TestConfigurationCreateRequest`; `PUT /configurations/{id}` →
  `TestConfigurationUpdateRequest`.

There is deliberately **no** `TestCaseCreateRequest` and no `TestSuiteCreateRequest`. Suites and cases are never
created by a flat route: they are created or placed through the composition routes
(`POST /projects/{id}/test_suites`, `POST /projects/{id}/test_cases`, `POST /test_suites/{id}/test_cases`), whose
body is the create/place union `CompositionRequest` (Issue #66). A test case's identity is its `testCaseId`, so
there is no partial "create a case from a name" arm to type.

Each create schema requires exactly the field its identifier is derived from — `name` for projects, runs,
milestones and configurations — which is what the create route already enforces. The update schemas require
nothing, because a `PUT` is the partial merge Issue #80 records, and an empty object is a valid partial payload.
All ten carry `additionalProperties: false`, because `validate_payload` rejects a body naming a field the
resource does not define (Issue #71) and type-checks every field it does (Issue #121); the document now
advertises that rejection instead of an open object.

The write schemas are the read model's properties, so a field keeps the description the read schema publishes
unless the write route behaves differently. Four descriptions are overridden to say what the write does:

- `projectId` and `testRunId` are **derived from `name`** as `<name>.json` when the body omits them, and
  `milestoneId` defaults to `name` — the identity normalisation Issue #78 records. `configId` is **always**
  derived, so a value the body supplies is accepted for wire compatibility and ignored (Issue #288).
- `testRunId` and `timestamp` on `TestRunCreateRequest` record that a run written from a name alone still gets a
  `timestamp`: the API stamps the current Unix-seconds string when the body omits one, and a supplied value is
  stored verbatim.
- `testSuites` on the project schemas and `testCases` on the suite schema are **accepted for wire compatibility
  and discarded**: membership lives in the folder tree, not in the parent document (Issue #65).
- `TestCaseUpdateRequest` needs no override — the read descriptions already say that a client-supplied `version`
  or `lastModified` is ignored (Issues #90 and #92) — and the request arms that mint those fields are not the
  body's.

`ProjectCreateRequest` and `ProjectUpdateRequest` therefore drop the response-only `testCases` field, because no
write stores it (Issue #65).

### Success responses (G4)

The shared response components `CreateResponse` (`{message, id}`, both required), `MessageResponse`
(`{message}`, required) and `UploadResponse` (`{message, filename, originalName, size}`, all required) are added,
and every `2xx` answer that had no body now `$ref`s the one that matches:

- **`CreateResponse`** — the four path-keyed creates (`POST /projects`, `POST /test_runs`, `POST /milestones`,
  `POST /configurations`) and the five shared `x-duplicate*` fragments the duplicate routes are documented with.
- **`MessageResponse`** — the 22 update, delete, composition, record, configuration-link and attachment-delete
  answers enumerated in `tests/service.rs`.
- **`UploadResponse`** — the two attachment uploads.

Four responses that already carried these shapes inline now `$ref` the shared components, so the shape is
written once: the defect-link `201` (→ `CreateResponse`, which drops its redundant inline
`additionalProperties: false`), the defect-unlink `200` (→ `MessageResponse`), and both attachment-upload `201`s
(→ `UploadResponse`). The defect *listing* stays inline: it answers `{"defects": [...]}` and is not one of the
three shapes.

**The count is 31 = 26 path-keyed operations + 5 shared `x-duplicate*` fragments.** The five duplicate operations
are documented as `$ref`s to the `components` fragments rather than as inline path items, so a fragment is what a
caller resolves and what had to be typed; the earlier figure of 26 counted only the path-keyed operations and
omitted the duplicate routes. Three `2xx` answers deliberately keep no JSON body: `GET /health`,
`GET /openapi.json` and `GET /api-docs`, which are not JSON operations. The `200` on
`GET /test_cases/{id}/attachments/{filename}` was already typed — `application/octet-stream`, a binary body — so
it is not one of the 31. (Issue #291 left that media type as it was and added the `Content-Disposition` header
to the three download `200`s.)

`tests/service.rs` holds the invariant generically: every `2xx` response outside those three routes declares a
non-empty `content` and a `schema` on every media type it publishes, so a future operation that forgets its body
fails the suite.

### Error codes (G8)

`Error.error.properties.code` now enumerates the eight codes the service publishes — `invalid_id`,
`invalid_request`, `invalid_status`, `invalid_multipart`, `missing_file`, `not_found`, `conflict`,
`storage_error` — each of them already recorded as its own response component under Issue #76. `Error` itself
stays free of `additionalProperties`, because the envelope is not a body a caller submits and
`tests/service.rs::openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields` holds it to the
permissive set.

### Deviation recorded

Documentation-only, held by tests that fail against the untyped document: the generic `2xx`-content invariant and
the ten write-body `$ref` assertions in
`tests/service.rs::openapi_documents_the_error_contract_of_every_operation` (the bare bodies and the missing
content are exactly what it now rejects), the response-component shapes and the `code` enum in
`tests/service.rs`, and
`tests/tags.rs::the_tags_parameter_is_published_only_where_a_tag_can_be_stored`, whose schema discovery now also
finds the write bodies that legitimately accept a `tags` array (`ProjectCreateRequest`, `ProjectUpdateRequest`,
`TestCaseUpdateRequest`, `TestRunCreateRequest`, `TestRunUpdateRequest`, `TestSuiteUpdateRequest`).

## Operation Metadata Plan (Issue #145)

Issue: [#145](https://github.com/TucanoTechnology/TucanoTestAPI/issues/145) — the document named no operation
(`operationId`) and grouped nothing by resource, so a generated client fell back to anonymous, path-derived
method names (`postProjectsIdDuplicate`) and could not tell which call belonged to which resource. It also
documented the five `POST … /duplicate` routes as `$ref`s to `components` fragments, which OpenAPI 3.0.3 has no
`components.pathItems` section to resolve, and it published a single unspecified server. This plan adds a stable
`operationId` and exactly one resource tag to every operation, inlines the duplicate path items, and replaces the
server with a templated one. It is documentation-only: no status code, response body, validation rule, route or
stored document changed. It is additive in the sense the client contract cares about — every route and every
schema is the same — and is the last piece the generated-client work in
[the GUI client boundary](../architecture/gui-client-boundary.md) needs to name its methods and group them.

### Stable operation ids (G1)

Every operation now carries an `operationId` derived from the route it documents, not from the path string.
The rule is `<verb><Resource>` in lower camel case: the verb is the HTTP method (`get`, `list`, `create`,
`update`, `delete`, `duplicate`, `add`, `remove`, `record`, `upload`, `download`, `import`, `link`, `unlink`), and
the resource is the noun the route acts on, pluralised for a collection (`listProjects`, `getProject`,
`createProject`, `duplicateProject`). Sub-resource routes keep the parent in the name so the grouping is legible
without the path: `listProjectTestSuites`, `addProjectTestCase`, `removeProjectTestSuite`,
`removeTestRunConfiguration`, `getMilestoneProgress`, `getCoverageReport`. The ids are unique across the
document and every one is a client-safe identifier — they begin with an ASCII letter, which is what most
generators require to derive a method name.

There are 64 operations, named as follows:

- **Service** — `getHealth`, `getOpenApiDocument`, `getApiDocs`.
- **Projects** — `listProjects`, `createProject`, `getProject`, `updateProject`, `deleteProject`,
  `duplicateProject`, `listProjectTestSuites`, `addProjectTestSuite`, `removeProjectTestSuite`,
  `listProjectTestCases`, `addProjectTestCase`, `removeProjectTestCase`.
- **TestSuites** — `getTestSuite`, `updateTestSuite`, `deleteTestSuite`, `duplicateTestSuite`,
  `listTestSuiteCases`, `addTestSuiteCase`, `removeTestSuiteCase`.
- **TestCases** — `getTestCase`, `updateTestCase`, `deleteTestCase`, `duplicateTestCase`,
  `uploadTestCaseAttachment`, `downloadTestCaseAttachment`, `deleteTestCaseAttachment`, `listStepAttachments`,
  `uploadStepAttachment`, `deleteStepAttachment`, `listTestCaseHistory`, `getTestCaseVersion`.
- **TestRuns** — `listTestRuns`, `createTestRun`, `getTestRun`, `updateTestRun`, `deleteTestRun`,
  `duplicateTestRun`, `addTestRunTestSuite`, `addTestRunTestCase`, `recordTestRunResult`, `listResultDefects`,
  `linkResultDefect`, `unlinkResultDefect`, `importJUnitResults`, `importJsonResults`, `addTestRunConfiguration`,
  `removeTestRunConfiguration`.
- **Milestones** — `listMilestones`, `createMilestone`, `getMilestone`, `updateMilestone`, `deleteMilestone`,
  `duplicateMilestone`, `getMilestoneProgress`.
- **Configurations** — `listConfigurations`, `createConfiguration`, `getConfiguration`, `updateConfiguration`,
  `deleteConfiguration`.
- **Reports** — `getCoverageReport`, `getSummaryReport`.

### Resource-family tags (G2)

The document declares eight top-level tags in the order above, one per resource family, and every operation
carries exactly one of them. A route is tagged by the resource it lives under, so the composition routes are
tagged by their parent — `POST /projects/{id}/test_suites` is `Projects`, not `TestSuites` — which is the same
rule the `operationId` prefix follows. The `Service` tag holds the six non-resource routes (`GET /health`,
`GET /ready`, `GET /diagnostics`, `GET /metrics`, `GET /openapi.json`, `GET /api-docs`).

### Duplicate operations inlined (G5)

The five `POST … /duplicate` routes were documented as `{"$ref": "#/components/x-duplicate*"}`, a `$ref` that
resolves to a *path item*. OpenAPI 3.0.3 has no `components.pathItems` section, so the reference was not
portable; a tool that only implements 3.0 either ignored it or refused the document. The same fragments were
reached by the `x-duplicate*` components, which the enumeration in `tests/service.rs` counted a second time,
inflating the operation count. Both are fixed by inlining each fragment into its path item and deleting the
five `x-duplicate*` components. The document now has 42 path keys — 37 concrete routes plus the five duplicates,
which are still five distinct path keys because a duplicate answers its own path — and no `x-` key anywhere under
`components`. Every one of the 428 `$ref`s in the document resolves.

### Deployment servers (G6)

The single server becomes a templated `{"url": "{scheme}://{host}:{port}"}` with three variables: `scheme`
(default `http`, enumerated `http` / `https`), `host` (default `localhost`) and `port` (default `3000`). The
defaults resolve to `http://localhost:3000`, the same origin the old server published, so a client generated
without overriding the variables reaches the same place; a deployment behind a TLS terminator sets `scheme` to
`https` and a client pointed at another environment overrides `host` and `port`. Each variable carries the
`description` a caller needs to make that choice.

### Deviation recorded

Documentation-only, held by three new tests that fail against the untagged document.
`tests/service.rs::openapi_operations_carry_stable_ids_and_resource_tags` asserts the eight declared tags, that
every operation carries exactly one declared tag, that every `operationId` is unique and client-safe, and pins
the count at 64. `tests/service.rs::openapi_inlines_the_duplicate_operations_and_drops_their_fragments` asserts
each duplicate path item is concrete (no `$ref`, a named `post` operation) and that no `x-` component remains.
`tests/service.rs::openapi_servers_describe_the_deployment_with_variables` asserts the single templated server
uses each declared variable and gives it a string default, including `host` and `port`. The pre-existing
`::openapi_document_matches_the_registered_routes`,
`::openapi_documents_the_error_contract_of_every_operation` and
`::openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields` stay green, and every `$ref` still
resolves, so no schema or route changed.

## Project-Scoped Runs, Milestones and Configurations Plan (Issue #215)

Issue: [#215](https://github.com/TucanoTechnology/TucanoTestAPI/issues/215) — `test_runs/<id>.json`,
`milestones/<id>.json` and `configurations/<id>.json` are the last three resources stored in a root-level
collection, which the real-home rule of Issue #65 forbids for everything else. This section records the
compatibility surface of the change; the decision, its alternatives and its consequences are in
[`docs/architecture/adr-storage-layout-v3.md`](../architecture/adr-storage-layout-v3.md), which is the
authority. **Status: decided, not yet implemented** — the layout the section describes is layout v3.

### Stored layout

The three collections move one level down, into the project folder that owns them, and stay single documents
rather than becoming folders:

```text
<data>/projects/<project>/test_runs/<id>.json
<data>/projects/<project>/milestones/<id>.json
<data>/projects/<project>/configurations/<id>.json
```

`Resource::ROOT_DIRS` becomes `[Projects]`, so `projects/` is the only collection below the data root (beside
`auth/` and `.tucano.lock`). **No document changes shape**: no field is added to `TestRun`, `Milestone` or
`TestConfiguration`, the home is the folder and is never stored, so the legacy Draft 2020-12 schemas are
untouched and `SUPPORTED_FORMAT_VERSION` stays `1`
([`file-format-versioning-plan.md`](./file-format-versioning-plan.md)).

### Identity

Run, milestone and configuration ids become unique **within a project**, matching suites. A global document
route therefore answers `404 not_found` for zero occurrences, operates on the single occurrence, and answers
`409 conflict` — through the existing `ambiguous()` helper, naming the parent-scoped routes — for two or more.
Global listings stay de-duplicated and sorted. `test_runs`, `milestones` and `configurations` are reserved
child names in a project folder, so a suite or case folder cannot take one.

### Wire surface

- **Added and documented:** `GET`/`POST /projects/{id}/test_runs`, `DELETE
  /projects/{id}/test_runs/{run_id}`, and the `milestones`/`{milestone_id}` and `configurations`/`{config_id}`
  siblings — nine operations. Each list answers a bare sorted id array and publishes no query parameter; each
  create answers `201 {"message","id"}`; each delete answers `200 {"message"}`. Unknown project → `404
  not_found`; duplicate id in that project → `409 conflict`; missing required fields → `400 invalid_request`.
- **Retired:** `POST /test_runs`, `POST /milestones`, `POST /configurations`. Each path stays registered and
  answers `400 invalid_request` naming its replacement, exactly as `POST /test_suites` and `POST /test_cases`
  do since Issue #66. The `TestRunCreateRequest`, `MilestoneCreateRequest` and
  `TestConfigurationCreateRequest` schemas the Issue #140 plan bound them to move to the parent-scoped creates
  unchanged, so no request body shape is retired with them.
- **Undocumented but served:** because the route-parity test compares path keys, the three bare collection
  paths join `api::UNDOCUMENTED_ROUTES` as whole keys, so the global scans `GET /test_runs`, `GET /milestones`
  and `GET /configurations` keep working and leave the published contract — the same trade Issue #66 made for
  `GET /test_suites` and `GET /test_cases`.
- **Unchanged paths:** `GET`/`PUT`/`DELETE /test_runs/{id}` and every run sub-route (suites, cases, results,
  defect links, imports, configuration links), `GET`/`PUT`/`DELETE /milestones/{id}`, `/duplicate`,
  `/progress`, and `GET`/`PUT`/`DELETE /configurations/{id}`. Their response shapes are unchanged; each gains
  `409 conflict` as a possible answer for an ambiguous identifier.
- **Operation count:** 68 → **71** (six removed, nine added). The pin in
  `tests/service.rs::openapi_operations_carry_stable_ids_and_resource_tags` moves with it.
- **Placement is not added.** A parent-scoped `POST` creates only: it accepts no `mode`, and a body carrying
  one is refused as an unknown field by the payload validation of Issues #71 and #121.

### Authorization

- A run requires the role in its **home project and in every project its `projects` array names** — `Viewer`
  to read, `Editor` to write. `TestRun.projects` keeps its shape and its meaning as the projects the run
  covered; it stops being the ownership source. The "a run naming no project is open to any authenticated
  caller" fallback is withdrawn.
- A milestone requires the role in its **home project and in every project its references reach** — `Viewer`
  to read, `Owner` to write. Because the home now supplies the project, the `400 invalid_request` "A milestone
  must reference at least one project" and the `403 forbidden` "This milestone is not linked to any project"
  are both withdrawn.
- A configuration becomes a project resource: `Viewer` to read, `Editor` to write or create, and
  `GET /configurations` is filtered to the caller's projects. The four installation-wide short-circuits in
  `src/api/access.rs` are removed. A run may still link a configuration from any project the caller reaches,
  which needs `Editor` there too, as `require_run_source` already does for suites and cases.
- A parent-scoped `DELETE` requires the role in the **named** project only and deletes that occurrence, so it
  works for an identifier another project also holds.
- References resolve with the acting document's home preferred, then globally: a milestone's `testRunIds` and a
  run's `configId` are looked up in the milestone's or run's own project first, and an identifier that is
  ambiguous after that answers `409 conflict` instead of resolving arbitrarily.

### Migration and rollback

No migration is performed or scripted. `FileRepository::new` creates only `projects/`, never reads the three
legacy root collections, and **refuses to start** when one of them still holds `*.json` documents (ignoring
`.tucano-*.tmp`), naming the offending directories and the manual recipe. It never deletes anything: an empty
legacy directory, which every pre-v3 volume has, is not an error. Rolling the image back against a v3 volume
leaves the older build answering `404` for these three resources and reporting empty milestone progress, so a
full rollback means restoring the pre-change snapshot the promotion runbook requires.

## Run Result Merge and Membership Plan (Issues #284, #285)

Issues: [#284](https://github.com/TucanoTechnology/TucanoTestAPI/issues/284) — re-recording a result for a case
the run already holds replaced the whole result, so a partial recording discarded the `notes`, `durationMs` and
`attachments` the previous one carried together with the defect links it had — and
[#285](https://github.com/TucanoTechnology/TucanoTestAPI/issues/285) — `POST /test_runs/{id}/results` validated
`status` and nothing else, so a run could hold a result for a case it never contained, and a malformed
`durationMs` / `notes` or an unknown field was silently dropped.

- **A recording merges into the result the run already holds.** `status` and `timestamp` are the two fields a
  recording always describes, so both are replaced by every request; `notes` and `durationMs` are replaced only
  when the body supplies them, so a re-recording that says nothing about either keeps the stored one. The stored
  `attachments` and `defectLinks` are never touched — a run stops losing the evidence attached to an earlier
  recording.
- **An explicit `null` clears a field; leaving the field out keeps the stored value.** This is a **route-local
  reading**, and the one place in the API where `null` does not mean "not supplied": `merged_document` (Issue
  #80, above) reads a `null` field as absent and keeps the stored value, and every `PUT` still does. The reading
  is kept because it is what this route has always done — the legacy `upsert_result` rebuilt the result from the
  body and took `notes` from `body.get("notes")`, so an explicit `null` cleared it — and because it is the only
  way a client can remove an optional field from a result. The deviation is stated here and in the
  `TestResultRequest` description in `openapi.json`.
- **A result can only be recorded for a case the run holds.** A run holds a case when it declares it — in the
  run's own `testCases`, or inside a suite the run embedded — or when it already records a result for it. Any
  other case answers `404 not_found` with "Test case not in test run" and nothing is written. The check runs
  after the identifier is parsed and after the run is resolved, so the validation order the error contract
  records is unchanged: an unusable run id is still `400 invalid_id` and an unknown run still `404` naming the
  run. Before this change a run accepted any case id, which is how `GET /test_runs/{id}` could list a result for
  a case the run never picked up.
- **Both importers stay outside the membership gate.** `POST /test_runs/{id}/import/junit` and
  `.../import/json` deliberately do not consult the run's declared population: an external report names the
  cases it ran, and importing it is how those cases come to be part of the run — gating an import would refuse
  exactly the reports the routes exist to read. An import still never overwrites a result the run already
  records.
- **The body is validated rather than read field by field.** The results route reads its body the way a create
  body is read: the body must be a JSON object, an unknown field is `400 invalid_request` naming it (so the
  `comment` and `duration` names a client may copy from the GUI ticket are rejected rather than dropped), a
  missing `testCaseId` or `status` is `400`, an empty one is `400`, a `status` outside the five is
  `400 invalid_status`, a `notes` that is not a string and a `durationMs` that is not a whole number are `400`,
  and a `timestamp` that is neither a non-empty string nor `null` is `400`. Nothing is written when any of them
  is refused.
- **A `timestamp` is kept verbatim and falls back to now.** Storage always keeps it as a string, either Unix
  seconds or ISO-8601, and the route neither parses nor reformats it: the value the body supplied is stored as
  written, and an omitted (or explicitly `null`) `timestamp` becomes the current Unix seconds. That is the
  format `TestCaseResult.timestamp` has always been documented with, so the field's contract does not change.
- **The case version a recording pins falls back to 1.** Recording a result still pins the version of the case
  the store holds; a case the run holds but the store no longer carries — a document removed after the run
  captured it — is pinned at version 1 rather than left unversioned.
- **`TestResultRequest` is published strictly.** `openapi.json` gains `additionalProperties: false`,
  `minimum: 0` on `durationMs`, `nullable: true` on `timestamp` / `notes` / `durationMs`, and a description
  stating the merge, the explicit-`null` reading and the membership rule, so the schema advertises the
  rejection the route now performs. The results operation also carries a description saying it updates any
  result the run already recorded for that case.
- Deviation recorded with tests in
  `tests/runs.rs::a_re_recorded_result_keeps_what_the_request_leaves_out`,
  `::a_result_is_refused_for_a_case_the_run_does_not_hold`,
  `::a_result_body_is_checked_rather_than_read_field_by_field`,
  `::recording_a_result_pins_the_version_of_the_case_the_store_holds` (a run may hold a case the store no longer
  carries), the run-population declarations the other result tests now make
  (`tests/runs.rs::a_partial_update_keeps_the_fields_the_body_leaves_out`,
  `::test_runs_support_composition_execution_and_isolation`), the results importers, whose exemption is pinned
  by `::a_junit_report_counts_duplicates_and_leaves_them_alone` and
  `::a_json_import_counts_duplicates_and_leaves_them_alone`, and in `src/domain/composition.rs`
  (`::a_second_result_for_a_case_merges_into_the_first`,
  `::a_re_recorded_result_keeps_the_fields_the_request_leaves_out`, `::an_explicit_null_clears_a_stored_field`,
  `::a_new_result_stores_what_the_request_describes`,
  `::a_new_result_stores_nothing_for_a_field_the_request_leaves_out`,
  `::re_recording_keeps_the_defect_links_and_attachments_it_cannot_describe`,
  `::a_run_holds_a_case_it_declares_directly`, `::a_run_holds_a_case_one_of_its_suites_declares`,
  `::a_run_holds_a_case_it_already_records_a_result_for`) and `src/domain/service/tests.rs`
  (`::run_results_are_recorded_and_updated`, `::a_result_for_a_case_the_run_does_not_hold_is_not_found`,
  `::a_result_body_is_rejected_rather_than_read_field_by_field`,
  `::a_re_recorded_result_keeps_what_the_request_leaves_out`).

## Concurrent Write Durability Plan (Issue #323)

Issue: [#323](https://github.com/TucanoTechnology/TucanoTestAPI/issues/323) — audit finding `F-177-3`
(`docs/security/audit-s2-storage-and-filesystem.md`): the document write path ran its read-modify-write as
three separately locked steps, so two concurrent `PUT`s that read the same test-case version both claimed the
next one, the second revision snapshot was discarded silently, and both callers were answered `200` — a write
the API acknowledged could be lost without a trace. The finding suggests holding the advisory lock across the
read-modify-write **or** making the write conditional on the version the client read; both arms are the
contract, and this section records them.

- **The advisory lock spans the whole read-modify-write.** A `PUT` on any resource runs through
  `Repository::transform_at`, which holds the store-wide advisory lock from the read of the stored document,
  through the merge and — for a test case — the revision snapshot and version bump, to the atomic write of the
  new document. Concurrent updates to one document are therefore serialised and **every write acknowledged
  with `200` is durable**: N acknowledged updates to a test case produce exactly N new versions, N history
  entries and N revision snapshots, and every acknowledged value reads back afterwards, from the live document
  or from a snapshot (`GET /test_cases/{id}/history` and `GET /test_cases/{id}/history/{version}`). No request
  that used to succeed is refused and no stored document shape changed; the observable difference is that an
  acknowledged write is no longer lost.
- **A write can be made conditional on the document the client read** (optimistic concurrency, Issues
  #263/#266, recorded here because it is the conflict response this plan's finding asks for). `GET` on a single
  document answers an `ETag` header holding a content hash of the stored bytes. A `PUT` may send `If-Match`
  with that value: the comparison happens inside the same lock, and a mismatch answers
  **`412 Precondition Failed`** with the standard error envelope (`code: "conflict"`, message "The document was
  modified by another request. Re-read and retry.") and the current `ETag` in a response header, so the client
  can re-read and retry. A `PUT` without `If-Match` keeps the legacy last-writer-wins behaviour — it overwrites
  the fields it carries and, as above, is never silently discarded. The `412` is additive: it only answers a
  request carrying `If-Match`, a header no earlier build examined. The finding named `409` for this arm; the
  implementation answers `412`, the status RFC 9110 reserves for a failed `If-Match`, and `409` keeps the
  meaning it has elsewhere in this contract (a duplicate creation or an ambiguous identifier).
- **The header surface is not yet published in `openapi.json`.** The `ETag` response header, the `If-Match`
  request header and the `412` answer predate this plan (they landed with Issues #263/#266) and are served but
  undocumented in the published contract; publishing them is left to a follow-up so this remediation stays
  scoped to the durability invariant and its regression proof.
- Deviation recorded with tests in
  `tests/security_tests.rs::data_integrity_tests::test_every_acknowledged_write_is_readable_back` (the
  `F-177-3` regression: 16 concurrent acknowledged writers, then the version, the history and every distinct
  acknowledged value are asserted readable back) and `tests/concurrency.rs`
  (`::test_concurrent_updates_return_412_on_etag_mismatch`, `::test_concurrent_creates_all_succeed`,
  `::test_concurrent_mixed_read_write`, `::test_lock_contention_latency`).

## Breaking change accounting

- **`ETag`, `If-Match` and the `412` precondition answer** (Issues #263/#266, recorded under the Concurrent
  Write Durability Plan above). `GET` on a single document gained an `ETag` response header, `PUT` accepts an
  optional `If-Match` request header, and a `PUT` whose `If-Match` does not match the stored document answers
  `412` with the current `ETag`. The change is additive: no request that used to succeed is refused, a `PUT`
  without `If-Match` behaves as before — except that its acknowledged write is now durable, which is the point
  of the plan — and no stored document shape changed. `openapi.json` does not yet publish the headers or the
  `412`; the follow-up is named in the plan.
- **Request id propagated, echoed and published in the error envelope** (Issue #106, plan above). `X-Request-Id`
  is a new response header on every answer, and the error envelope gains an optional `requestId`. The change is
  additive: no request that used to succeed is refused, no stored document changes, and an envelope a client
  reads only for `code` / `message` is unchanged apart from the extra key. The id is a correlation handle, not a
  secret — it is minted from a process-local random state and a counter, and an inbound header is accepted
  verbatim up to the empty/visibility checks. Deviation recorded with tests in `tests/request_id.rs` as listed in
  the plan.
- **Coverage report endpoint added** (Issue #94, plan above). `GET /reports/coverage` and the `CoverageReport` /
  `SuiteCoverage` schemas are new; nothing is written and no stored document shape changed, so the change is
  additive. The two schemas are permissive (no `additionalProperties`), and the new operation publishes no
  request body. Two semantics are recorded here: `totalCases` includes the cases a project holds directly, so it
  can exceed the sum of the per-suite `caseCount`; and `projectId` is echoed only when the report was scoped,
  which is why it is absent from the schema's `required` list. Deviation recorded with tests in `tests/reports.rs`
  and `src/domain/reports.rs` as listed in the plan.
- **Summary report endpoint added, with an optional result duration** (Issue #95, plan above).
  `GET /reports/summary` and the `SummaryReport` schema are new, and `TestCaseResult` / `TestResultRequest`
  gain an optional `durationMs`. The change is additive: no payload that used to succeed is refused, an
  existing document is never rewritten, and a stored result keeps its on-disk shape unless a client supplies a
  duration. Two semantics are recorded here: `total` counts every result, including `Retest` and unrecognised
  statuses, so it can exceed the sum of `passed` / `failed` / `blocked` / `untested`; and `passPercentage` is
  the exact fraction, not a rounded percentage. Deviation recorded with tests in `tests/reports.rs` and
  `src/domain/reports.rs` as listed in the plan.
- **Milestone progress derives one population, so the counts change** (Issue #286, note above). No field,
  response shape, schema or route changes: `MilestoneProgress` keeps its keys and `GET /milestones/{id}/progress`
  keeps its path. What changes is the arithmetic behind them — `totalCases` and the five buckets now come from a
  single deduplicated population per run, where before they counted two populations and could disagree (and
  `passPercentage` could exceed `100`). A milestone whose runs record results beyond their snapshot therefore
  reports different, now internally consistent, numbers: this is the first deliberate departure from the legacy
  handler's arithmetic since layering, wire-compatible in shape but not in values, so a client asserting on the
  old counts must be updated. Deviation recorded with tests in `tests/milestones.rs`,
  `src/domain/progress.rs` and `src/domain/service/tests.rs` as listed in the note above.
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
  **Amended by Issue #289 (entry below):** the download route this ticket ruled out arrived later as its own
  additive change, and only in the bare form — see its entry under breaking change accounting.
  Deviation recorded with tests in `tests/cases.rs`, `tests/attachments.rs` and `tests/service.rs` as listed in
  the plan.
- **New optional `testCases` on assembled project responses** (Issue #65). Legacy Draft 2020-12 `Project`
  documents do not know this field; it appears only when a project directly owns cases. The GUI is updated to
  read it; legacy clients that reject unknown fields fail loudly only for projects with direct cases, which
  previously could not exist through this API.
- **Storage layout v2** replaces the flat `projects/`, `test_suites/`, `test_cases/` layout. Existing data
  directories created by earlier builds are development artifacts and are not migrated; fresh layout is created
  on startup. Persistence behaviour is asserted by repository unit tests that inspect the physical tree, and by
  the integration suites. **Amended by layout v3 (Issue #215, plan above):** the same no-migration policy
  applies to `test_runs/`, `milestones/` and `configurations/`, which v3 moved inside the project folder, with
  one difference — `FileRepository::new` now refuses to start when a legacy root collection still holds
  documents rather than leaving them silently unread.
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
- **Test-case versions added** (Issue #90, plan above). `TestCase` gains optional `version` and `lastModified`
  fields that the API manages, and a qualifying update now writes a `revisions/v{version}.json` snapshot of the
  previous document. Additive for stored data and for reads: a case written before the issue keeps its exact
  shape and is never rewritten, and a case the API has since versioned simply carries two extra keys. The
  loosening is that a case payload supplying `version` or `lastModified` used to be refused as an unknown field
  and is now accepted and ignored — the API owns those keys, so the value a client sends has no effect. No
  request that used to succeed is refused. Deviation recorded with tests in `tests/cases.rs` as listed in the
  plan.
- **Test-case revision history endpoints added** (Issue #91, plan above). `GET /test_cases/{id}/history` and
  `GET /test_cases/{id}/history/{version}`, the `CaseHistoryEntry` schema, the `version` path parameter and the
  `InvalidVersion` `400` response are new; no stored document shape changed and no response that existed before
  was altered, so the change is additive. Two decisions are recorded here. First, the listing is a **bare
  array** — `[{"version", "lastModified"?, "changedFields"}]`, oldest first — as the normative versioning plan
  specifies, not the `{"versions": […]}` wrapper the issue text sketched; the wrapper was redundant, and the
  plan is the document the contract is held to. Second, `changedFields` names the qualifying fields (`title`,
  `steps`, `preconditions`, `expectedResult`) in which a snapshot differs from the version that superseded it,
  so the newest snapshot is measured against the live document; the live version is not a snapshot and never
  appears, and a case that has never had a qualifying update lists `[]`. `lastModified` is optional on the
  entry because a snapshot written before versioning carries no stamp. The revision route resolves the case
  before it parses the version, so an unknown case answers `404` even when the version is unusable; an unusable
  version on a known case is `400 invalid_request`, and an unrecorded version — including the current live
  version, which is read through `GET /test_cases/{id}` — is `404`. Deviation recorded with tests in
  `tests/cases.rs` (`::a_case_without_qualifying_updates_has_an_empty_history`,
  `::case_history_lists_its_snapshots_oldest_first_with_the_fields_they_changed`,
  `::a_recorded_revision_is_returned_verbatim_and_the_live_version_is_not_a_snapshot`,
  `::history_resolves_the_case_before_it_validates_the_version`,
  `::history_is_available_for_a_case_held_inside_a_suite`, and
  `::a_snapshot_written_before_versioning_lists_without_a_timestamp`), with
  `tests/service.rs::openapi_schemas_are_strict_only_where_the_api_rejects_unknown_fields` holding
  `CaseHistoryEntry` to the permissive set, and `::openapi_document_matches_the_registered_routes` proving both
  paths are published and registered.
- **Test-case versions captured in runs** (Issue #92, plan above). `TestRun` gains an optional `caseVersions`
  map, written when a case is added to a run or a result is recorded. Additive for stored data and reads: a run
  written before the issue keeps its exact shape and is never rewritten by a read, and a run the API has since
  versioned simply carries one more key. The loosening mirrors #90 — a run payload supplying `caseVersions` used
  to be refused as an unknown field and is now accepted, so no request that used to succeed is refused.
  Deviation recorded with tests in `tests/runs.rs` and `src/models.rs` as listed in the plan.
- **Write bodies, success responses and error codes typed in `openapi.json`** (Issue #140, plan above).
  Documentation-only: the ten JSON write operations now `$ref` named request schemas with
  `additionalProperties: false`, the 31 body-less `2xx` answers (26 path-keyed operations plus the five shared
  `x-duplicate*` fragments) now `$ref` `CreateResponse` / `MessageResponse` / `UploadResponse`, and
  `Error.error.properties.code` enumerates the eight published codes. No status code, response body, validation
  rule or stored document changed; the write schemas narrow an open object to the rejection the service already
  performs (Issues #71 and #121) and to the required fields the create routes already enforce. A consumer that
  generated a client from the old document may see narrower request types it previously treated as free-form,
  and the error envelope's `code` may now be modeled as an enum; no previously published success response
  changed. Deviation recorded with tests in `tests/service.rs` and `tests/tags.rs` as listed in the plan.
- **Operation metadata added to `openapi.json`** (Issue #145, plan above). Documentation-only: all 64 operations
  gain a stable `operationId` and exactly one resource-family tag from the eight now declared; the five
  `POST … /duplicate` path items are inlined instead of `$ref`-ing `components` fragments, and the five
  `x-duplicate*` components are removed; the single server becomes `{scheme}://{host}:{port}` with documented
  defaults that resolve to the previous `http://localhost:3000`. No status code, response body, validation rule,
  route or stored document changed, and every `$ref` still resolves. A consumer that generated a client from the
  old document may see renamed methods (the path-derived names such as `postProjectsIdDuplicate` become
  `duplicateProject`) and one client class per tag. The count is 64 operations — the issue text said 60, but the
  five duplicates were being counted twice by the old `$ref` fragments; the new test pins the honest 64.
  Deviation recorded with tests in `tests/service.rs` as listed in the plan.
- **Runs, milestones and configurations stored inside their project** (Issue #215, plan above). Storage layout
  v3: `<data>/test_runs/<id>.json`, `<data>/milestones/<id>.json` and `<data>/configurations/<id>.json` become
  `<data>/projects/<project>/test_runs/<id>.json` and siblings, and their identifiers become unique per project
  instead of globally. **No stored document changes shape** — the home is the folder and is never written into
  the document, so no field is added to `TestRun`, `Milestone` or `TestConfiguration`, the legacy Draft 2020-12
  schemas are untouched, and `SUPPORTED_FORMAT_VERSION` stays `1`. The wire change is not additive and has five
  parts. (1) `POST /test_runs`, `POST /milestones` and `POST /configurations` no longer create: they answer
  `400 invalid_request` naming the parent-scoped replacement, and the three bare collection paths leave
  `openapi.json` as whole keys — which also withdraws the published `GET /test_runs`, `GET /milestones` and
  `GET /configurations` scans, although the router keeps serving them, exactly as Issue #66 did for
  `/test_suites` and `/test_cases`. (2) Nine operations are added: `GET`/`POST` on
  `/projects/{id}/test_runs`, `/projects/{id}/milestones` and `/projects/{id}/configurations`, and `DELETE` on
  each one's `/{run_id}`, `/{milestone_id}`, `/{config_id}` item path, taking the documented count from 68 to
  71. (3) Every global document route for the three resources — including the run sub-routes, `/duplicate` and
  `/progress` — can now answer `409 conflict` for an identifier two projects hold, naming the parent-scoped
  routes, which is the answer `/test_cases/{id}` already gives. (4) Authorization changes: a run now needs the
  role in its home project **in addition to** every project its `projects` array names, so the "a run naming no
  project is open to any authenticated caller" fallback is withdrawn; a milestone needs it in its home in
  addition to its references, and because the home supplies a project the `400` "A milestone must reference at
  least one project" and the `403` "This milestone is not linked to any project" are both withdrawn, making a
  reference-less milestone legal; configurations stop being installation-wide and need `viewer` to read and
  `editor` to write in their home project, so `GET /configurations` becomes filtered for a restricted caller.
  (5) Report and listing scope **widens** for runs, because the home is now the ownership fact: `?projectId=`
  on `GET /reports/summary` also matches a run stored in that project whose `projects` array omits it, and a
  run naming no project at all is reachable — so it is counted by the report and listed for a restricted
  caller — whenever its home is, where the withdrawn "must name at least one project" rule hid it. The summary
  report also stops computing over the de-duplicated global listing and walks projects instead, so two runs
  sharing an identifier in different projects are now **both** counted where one was silently dropped.
  Nothing is migrated: a volume holding documents in a legacy root collection makes `FileRepository::new` fail
  at startup with a message naming the directories and the manual recipe, and the service never deletes them.
  Rolling the image back against a v3 volume answers `404` for these resources and reports empty milestone
  progress, so a full rollback means restoring the pre-change snapshot. Deviation recorded with tests in
  `src/storage/layout.rs`, `src/storage/fs.rs`, `tests/runs.rs`, `tests/milestones.rs`,
  `tests/configurations.rs`, `tests/reports.rs`, `tests/auth.rs` and `tests/service.rs` as listed in the plan.
- **Run results merge, and only for a case the run holds** (Issues #284 and #285, plan above). The results route
  becomes **restrictive** where it used to be permissive, which is the one place this change is not additive:
  `POST /test_runs/{id}/results` now refuses a case the run does not hold (`404 not_found`), a body carrying an
  unknown field, a `status` or `testCaseId` that is missing or empty, a `status` outside the five, a `notes`
  that is not a string, a `durationMs` that is not a whole non-negative number, and a `timestamp` that is
  neither a non-empty string nor `null` — where every one of those used to be accepted and silently dropped. A
  client that recorded a result for a case it had not added to the run must add the case (or import the report
  that names it) first. Every other request succeeds exactly as it did; the merge is additive for stored data,
  in that a re-recording that used to discard `notes`, `durationMs`, `attachments` and `defectLinks` now keeps
  them, while the explicit `null` that used to clear `notes` still clears it. No stored document is rewritten
  and no field changed shape. The one documentation change is `TestResultRequest` joining the strict schemas.
  Deviation recorded with tests in `tests/runs.rs::a_re_recorded_result_keeps_what_the_request_leaves_out`,
  `::a_result_is_refused_for_a_case_the_run_does_not_hold`,
  `::a_result_body_is_checked_rather_than_read_field_by_field`, and the `src/domain/composition.rs` and
  `src/domain/service/tests.rs` unit tests listed in the plan.
- **A supplied `configId` is ignored and the configuration identity is always derived** (Issue #288, amending the
  Issue #78 identity plan above). The change is confined to one field of one document type: a create body may
  still carry `configId` — the field stays in the schema and is accepted — but the stored value is always the
  `<name>.json` the document is listed under, because a
  `configId` is resolved to the file that holds it and a value that disagreed with its own document would name
  nothing. A client that supplied a **differing** `configId` used to read it back out of the document and now
  reads the derived one, and the traversal-shaped value that path used to persist beside a safe listing key is
  gone. No field is added or removed, the legacy Draft 2020-12 configuration schema is untouched, every other
  resource keeps the verbatim rule, and a document already on disk that holds a disagreeing `configId` is not
  rewritten by a read — the next write derives it. Deviation recorded with tests in
  `tests/configurations.rs::a_supplied_config_id_is_ignored_in_favour_of_the_derived_id`, the three existing
  configuration assertions that now expect the derived id (`::configurations_support_the_full_crud_lifecycle`,
  `::configuration_markers_are_plain_json_under_the_data_root`,
  `::a_partial_update_keeps_the_fields_the_body_leaves_out`), and
  `src/domain/service/tests.rs::normalise_marker_configuration_takes_the_id_over_a_supplied_identity`.
  **Amended by Issue #300 (entry below):** the update half of this is a refusal rather than an acceptance now —
  a `PUT` body whose `configId` names another document answers `400 invalid_request` and one the store cannot
  file answers `400 invalid_id`, so a differing `configId` on an update no longer succeeds and is no longer
  ignored, which overturns `::an_update_cannot_move_a_configuration_identity_away_from_its_id` and is recorded
  by `::an_update_refuses_a_body_identifier_that_names_another_configuration` and
  `::an_update_refuses_a_body_identifier_the_store_cannot_file` in its place. The create half stands unchanged.
- **Parent-scoped attachment routes added** (Issue #290). Twelve operations are added and none is withdrawn:
  the case- and step-attachment families each gain a form addressed through the holding project
  (`/projects/{id}/test_cases/{case_id}/…`) and a form addressed through the holding suite
  (`/test_suites/{id}/test_cases/{case_id}/…`), taking the documented count from 73 to 85. Purely additive:
  no bare path, method, parameter or response shape changed, no schema gained or lost a field, no stored
  document shape changed, and no request that used to succeed is refused — the new routes exist to answer
  where the bare ones cannot, namely a case id that copy-on-include placed under two parents, which the bare
  routes still refuse with `409 conflict`. The one recorded asymmetry is deliberate: a step attachment has no
  download route in any form, bare or parent-scoped, so the new step routes are four upload/list/delete
  operations. **Amended by Issue #289 (entry below):** the bare step download route has since been added, a
  fifth operation in the step family; the parent-scoped step forms still have no download. One existing message
  does change wording: the `409 conflict` an ambiguous identifier raises now
  labels each home (`project billing.json`, `suite smoke.checkout.json in project payments.json`) instead of
  printing a bare path, so the list it prints agrees with the count it opens with — the second defect the issue
  reports — and, for a case id, it names the parent-scoped attachment routes among the ways to address one
  occurrence. Only the prose moved; the status code, the `conflict` code and the envelope are unchanged, so a
  client that reads `code` is unaffected. Deviation recorded with tests in `tests/attachments.rs`,
  `src/domain/service/tests.rs::the_conflict_names_the_parent_scoped_routes_of_its_own_resource` and
  `::the_conflict_labels_a_suite_home_so_the_list_counts`, and
  `tests/service.rs::a_parent_scoped_attachment_route_reads_its_parent_from_the_path`, which holds all twelve
  routes to `400 invalid_id` for an unusable parent, plus `::openapi_document_matches_the_registered_routes`
  and `::openapi_documents_the_error_contract_of_every_operation` for the published surface.
- **Downloads served opaquely and named for the client** (Issue #291, note above). The document already declared
  `application/octet-stream` on the three case-attachment downloads, so no declared media type changes; what
  changes is what the deployment serves. The `200` used to carry a content type derived from the stored name —
  `text/plain` for a `.txt` attachment — which the operation's own `Blob` typing contradicts: a generated client
  that picks its decoder from the response type read such a body as a `string`, so the browser download never
  started, and non-UTF-8 bytes were corrupted by the text decode. All three now answer
  `application/octet-stream` and add `Content-Disposition: attachment` naming the file the uploader supplied, so
  bytes and filename reach every client in one shape. `openapi.json` gains the `ContentDisposition` header
  component, references it from the three `200`s (four after #289), and describes the `Attachment` members — including that
  `mimeType` is metadata that never becomes a response content type. The case upload route additionally records
  `uploadedAt` (ISO-8601 UTC) on the entry it stores. Observable departures: a client that read the response
  content type now always sees `application/octet-stream` and must take the stored type from the case document's
  `mimeType`; the response gains a header; and a newly uploaded case attachment gains an `uploadedAt` key. No
  request that used to succeed is refused, no existing document is rewritten, `UploadResponse` keeps its four
  fields, `StepAttachment` gains nothing, and the `Attachment` required set is unchanged. Deviation recorded with
  tests in `tests/attachments.rs::attachment_downloads_are_opaque_and_named_for_the_client`,
  `::a_non_ascii_file_name_is_named_for_the_client`, `::an_upload_records_when_the_file_arrived`,
  `::a_step_attachment_records_no_upload_time`,
  `tests/service.rs::openapi_types_and_names_every_binary_download`, and the `src/domain/mod.rs` unit tests for
  `original_name` and `content_disposition`.
- **An update refuses a body identifier that would move the document** (Issue #300). The `PUT` routes for the
  project, suite, run, milestone and configuration answered `200 {"message":"Resource updated"}` to a body that
  supplied an identity field naming something else, and wrote it: for four of the five resources
  `normalise_marker` takes a supplied identity string as authoritative (Issue #78 above), so the stored
  `projectId` / `suiteId` / `testRunId` / `milestoneId` could disagree with the folder or file that holds the
  document, and a value the store could not file at all — bare, nested, empty, `.` or `..` — was accepted and
  ignored rather than answered with the `400 invalid_id` the document promises (Issue #288 had already recorded
  the configuration half of this as accepted-and-ignored). All five now answer before anything is written, in
  four arms: an absent field is normalisation's business as before; a value that restates the addressed
  identifier or the stored one is accepted; a value the store cannot file answers `400 invalid_id`; and a usable
  value naming another document answers `400 invalid_request`, with a message that says why — the identifier is
  the address, so it is immutable and a rename is a delete and a recreate. Refusing is the deliberate choice
  over performing the rename: the identifier names the folder or `<id>.json` file holding the document, the role
  grants that authorise the resource are keyed by that name under `auth/projects/`, the parent-scoped routes
  resolve it, and a run's results are keyed per case id — so a rename performed here would orphan grants and
  addresses that no other route moves with it. Observable narrowing: a differing `configId` on an update used to
  succeed and be ignored and now answers `400 invalid_request` (the Issue #288 amendment above), and a project,
  suite, run or milestone identity naming another document used to be written into the stored document and now
  answers `400 invalid_request`. Scope is those five routes; `PUT /test_cases/{id}` is unchanged, because a case
  identifier is addressed verbatim and its body `testCaseId` is a document field rather than an address, which
  is recorded as a follow-up in `docs/security/audit-s2-storage-and-filesystem.md` (`O-177-14`). The same
  question on the duplicate routes is consolidated to one answer: a body `newId` the store cannot file answers
  `400 invalid_id` on every duplicate route, where `POST /test_suites/{id}/duplicate` previously answered
  `400 invalid_request` for the same value (the API side of TucanoTestGUI #165). Each route's path identifier
  keeps the code it already answered — `invalid_request` on `POST /projects/{id}/duplicate`, `invalid_id` on the
  suite, run and milestone routes, and `404 not_found` on `POST /test_cases/{id}/duplicate`, which addresses a
  case verbatim and needs no `.json` suffix there, so a bare body `newId` on that route stays usable as it
  stands. Creation gains the rule its shipped schema already claimed: a `projectId` supplied to `POST /projects`
  must be a single path segment ending in `.json`, anything else answers `400 invalid_request`, and when usable
  it becomes the new project's identity and address. Before this a bare `projectId` was answered `201` under the
  derived `<name>.json` address and a nested one was silently flattened to its last segment, which closes the
  reported-versus-accepted divergence that `O-177-14` records for projects. One create-side asymmetry stands: a
  create body's `testRunId` is still stored verbatim while the run's address derives from `name`, the other half
  of `O-177-14`. `openapi.json` records the same rule on the five `PUT` identity fields, on the duplicate
  `newId` fields and their `400` references, and on `ProjectCreateRequest.projectId`. Deviation recorded with
  tests in `tests/projects.rs::a_supplied_project_id_names_the_new_project`,
  `::an_update_refuses_a_body_identifier_that_names_another_project`,
  `::an_update_accepts_a_body_identifier_that_restates_the_addressed_one`,
  `::an_update_refuses_a_body_identifier_the_store_cannot_file`, the same refusal pair per resource in
  `tests/suites.rs`, `tests/runs.rs`, `tests/milestones.rs` and `tests/configurations.rs`, and
  `tests/service.rs::duplicate_routes_refuse_a_body_new_id_the_store_cannot_file`.
- **The project-scoped suite, case and run listings filter by tag** (Issue #293). `GET
  /projects/{id}/test_suites`, `GET /projects/{id}/test_cases` and `GET /projects/{id}/test_runs` answered the
  complete child set and ignored the query string, so the GUI's project-scoped listings (TucanoTestGUI #119)
  could not use the `?tags=` filter the contract documents, even though `TestSuite`, `TestCase` and `TestRun`
  all carry `tags`; the gap is the deliberate #49/#122 case only for `milestones` and `configurations`, whose
  models define no `tags` field. All three listings now read `ListQuery` and honour the parameters their
  resource can answer: `?filter=` and `?tags=` on each, and the runs-only `?configuration=` on the run listing.
  This is additive. Without a parameter the listing is the same sorted id array it answered before, no request
  that succeeded now fails, no document changed shape, and a filter that matches nothing answers `[]` with
  `200` rather than `400`. The matcher is the one the global `GET /projects` scan has always used — a single
  shared implementation, not a second one — so `?tags=` keeps its documented semantics (comma-separated, each
  element trimmed, compared case-insensitively, keeping a resource that carries *at least one* of the requested
  tags and never matching a resource without a `tags` array) and composes with `?filter=` and `?configuration=`
  in the documented order, all conjunction. Two consequences are deliberate. One, a child is judged on the
  document the addressed parent holds rather than the first occurrence a global lookup resolves, so a suite or
  case identifier two projects hold is filtered as the occurrence that project owns — the occurrence the
  project-scoped read and delete already address. Two, `openapi.json` publishes `filter` and `tags` on both the
  suite and case listings and files `configuration` under `listProjectTestRuns` alone: a parameter the router
  honours has to be in the document ("if it is not in `openapi.json`, it does not exist"), and `configuration`
  stays off the suite and case listings on the same Issue #122 reasoning that withdrew `tags` from `milestones`
  and `configurations` — neither resource links a configuration. `GET /test_suites/{id}/test_cases`, the
  `milestones` and the `configurations` listings are unchanged and stay exhaustive, and `?tags=` on
  `GET /milestones` and `GET /configurations` is still accepted and ignored, as the Tags Plan above records.
  Deviation recorded with tests in `tests/tags.rs`: a project's suites, cases and runs each match
  case-insensitively and after trimming, keep the any-of semantics, never match an untagged child, answer `[]`
  for an unnamed tag, compose with `?filter=`, the run listing composes with `?configuration=`, one project's
  tag filter judges that project's occurrence rather than the global first one, and the document assertion pins
  `?tags=` to the four list operations whose resource can store one.
- **The bare step attachment downloads** (Issue #289, note above). One operation is added and none is
  withdrawn: `GET /test_cases/{id}/steps/{step_index}/attachments/{filename}` answers the stored bytes where the
  path previously answered `405 Method Not Allowed` with `allow: DELETE`, taking the documented count from 86
  to 87. Purely additive: no bare path, method, parameter or response shape changed, no schema gained or lost a
  field, no stored document shape changed, and no request that used to succeed is refused. The response mirrors
  the case-level download exactly — `Content-Type: application/octet-stream` whatever the stored file is, plus
  `Content-Disposition: attachment` naming the uploaded file (Issue #291 above) — so a client that decodes by
  response content type still receives bytes. `StepAttachment.mimeType` stays the description recorded in the
  step document and never becomes a response content type, and the entry gains no `uploadedAt`: that key belongs
  only to the case-level `Attachment` the Issue #291 upload records, and adding it to a step attachment would be
  a schema change no issue asks for under the `additionalProperties: false` rule. Two asymmetries with the
  collection route are recorded deliberately. A `{step_index}` that is not a non-negative integer answers
  `400 invalid_request` on every form of this path; a `{step_index}` that names no structured step answers
  `404 not_found` here, because the byte route reads the file the index constructs rather than validating the
  index against the case, while the list and upload routes on the same path answer `400`. And, as Issue #290
  above records, the parent-scoped step families still have no download form in either direction — this ticket
  adds only the bare route. Deviation recorded with tests in `tests/attachments.rs`, including
  `::a_step_attachment_downloads_opaquely_and_named_for_the_client`,
  `::a_missing_step_attachment_download_returns_not_found`, `::a_step_attachment_download_name_may_not_traverse`
  and `::step_attachments_reject_an_unusable_step_index`, plus
  `tests/service.rs::openapi_types_and_names_every_binary_download`, which holds the document to exactly four
  binary downloads, `::openapi_declares_the_security_posture_of_every_operation`,
  `tests/auth.rs::every_guarded_operation_refuses_an_anonymous_caller`, and the
  `identifiers_cannot_escape_the_storage_root` and `a_test_case_identifier_is_addressed_verbatim` matrices in
  `tests/service.rs`, which carry the new path.
- **A recorded result can be corrected and withdrawn** (Issue #283). `POST /test_runs/{id}/results` could
  record a result for a case but nothing could rewrite or take one back, so a mistyped result stayed in the run
  and the only way out was deleting the whole run. Two routes close the gap: `PUT
  /test_runs/{id}/results/{case_id}` replaces the result the run holds for that case, and `DELETE
  /test_runs/{id}/results/{case_id}` removes it. This is additive: the documented surface grows from 89 to 91
  operations, no existing operation, schema or field changed shape, and no request that succeeded now fails.
  The replacement body is the recording shape with the case named by the path, published as
  `TestResultReplaceRequest` (`additionalProperties: false`, `required: ["status"]`): `testCaseId` is optional
  and, when present, has to name the case in the path — a different identifier is `400 invalid_request` — while
  `status` is required, `timestamp` defaults to the current instant, and `notes` and `durationMs` are rewritten
  only when supplied, with `null` clearing them, exactly as the recording route treats them. A replacement
  rewrites an existing result and never creates one: a case the run holds no result for, or a run whose
  `results` is absent or empty, answers `404` "Test result not found in test run", and the run is read after the
  body is validated, so a replacement that cannot be applied writes nothing. What the body cannot carry is left
  alone: `attachments` and `defectLinks` survive a replacement, and, because both routes address the run
  document the request names, `DELETE` takes the result's attachments and links with it — `GET
  /test_runs/{id}/results/{case_id}/defects` then answers `404`. Both routes take the actor an editor of the
  run's project, share the results envelope (`200 {"message": ...}`), and answer `404` for an unknown run.
  Deviation recorded with tests in `tests/runs.rs::replacing_a_result_keeps_the_defects_it_cannot_describe`,
  `::removing_a_result_takes_it_out_of_the_run`, `src/domain/composition.rs`
  (`::a_replacement_rewrites_every_field_the_request_describes`,
  `::a_replacement_keeps_the_defect_links_and_attachments_it_cannot_describe`,
  `::replacing_an_absent_result_is_not_found`, `::removing_a_result_takes_it_out_of_the_run`,
  `::removing_an_absent_result_is_not_found`) and `src/domain/service/tests.rs`
  (`::a_result_is_replaced_by_the_case_the_route_addresses`,
  `::replacing_or_removing_an_absent_result_is_not_found`).

- **Release and environment listings added** (Issue #262). `GET /releases` and `GET /environments` are new
  guarded read operations answering `200` with a JSON array of strings — the distinct, byte-wise sorted `name`
  of every milestone, respectively test configuration, held by the projects the caller reaches. The change is
  additive: no route, method, parameter, response shape or stored document changed, and no field was added to a
  schema, so the legacy Draft 2020-12 shapes with `additionalProperties: false` are untouched and the addition
  needs no versioning plan. Two semantics are recorded here. One, the listings filter rather than refuse: a
  project outside the caller's grants contributes nothing, exactly as the other global and project-scoped
  listings behave, and the `403` the document publishes alongside `401` is the guard's own refusal rather than
  a consequence of partial reachability. Two, a name is read from the project that stores the document alone —
  `filter_list`'s configuration rule, and the home half of its milestone rule — not from the projects a
  milestone's `testSuiteIds` / `testRunIds` additionally reach, so a name two projects repeat is reported once
  and a name only an unreachable project holds is not reported at all. An installation holding no milestone,
  respectively no configuration, answers `[]` at `200` — the degrade path the GUI's context bar reads — never
  `404`. Deviation recorded with tests in `tests/metadata.rs`.

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
