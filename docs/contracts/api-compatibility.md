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
