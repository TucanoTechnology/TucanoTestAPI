# GUI Client Boundary and Generated-Client Strategy (Issue #99)

Issue: [#99](https://github.com/TucanoTechnology/TucanoTestAPI/issues/99) — define the GUI client boundary
and a generated-client strategy driven by [`openapi.json`](../../openapi.json). Split from #16. This is a
strategy/contract record: the GUI client itself lives in
[TucanoTestGUI](https://github.com/TucanoTechnology/TucanoTestGUI), which is currently paused.

The API/GUI parity rule in [`AGENTS.md`](../../AGENTS.md) says every action can be performed through the GUI
or directly over the API, and neither is secondary. Both call the *same* HTTP surface, so the GUI's client
must not be a second, hand-maintained copy of the contract — it is generated from the contract the service
already publishes.

## Decision

| Aspect | Decision |
| --- | --- |
| Single source of truth | [`openapi.json`](../../openapi.json). If an operation, parameter, request body, or response is not in it, it does not exist for any client. |
| Generated client | Produced from `openapi.json` by [OpenAPI Generator](https://openapi-generator.tech) via the official `@openapitools/openapi-generator-cli` wrapper, using the `typescript-fetch` generator (the GUI is TypeScript + React + Vite). |
| Where it lives | Committed under `TucanoTestGUI/src/api/generated/`. The API repository produces the contract; the GUI repository owns the generated output. |
| Boundary rule | No hand-written HTTP calls in the GUI. Every request and response shape comes from the generated types. |
| Cross-check | The contract tests required by [`AgentRules/test/contract.md`](../../AgentRules/test/contract.md), realized in this repository by the route-coverage assertion in `tests/service.rs` plus the per-area suites. |
| Alternative for Rust consumers | [`progenitor`](https://github.com/oxidecomputer/progenitor) generates a Rust client from the same document, should the service itself or a future Rust consumer need one. |

## The boundary

The GUI is a **client of the API and of nothing else**.

- It never reads or writes the storage directory; that is the API's alone (the storage concept in
  [`README.md`](../../README.md) and the "GUI to storage" boundary in
  [`docs/security/threat-model.md`](../security/threat-model.md)).
- It never duplicates the contract. Hand-written request paths, query parameters, or response interfaces are
  the failure mode this strategy removes: they drift silently the moment the API changes, and the drift is
  invisible to `cargo test` because it lives in another repository.
- Every type it compiles against is a generated type. Where the GUI needs a domain view — a form model, a
  table row — it derives it from generated types, it does not define a parallel one.

The GUI's own README and [`docs/architecture/rust-service-core.md`](rust-service-core.md) already state this
boundary ("The GUI must use the same documented API as other clients. It should not read the storage
directory directly."). This document fixes the *mechanism*: the API can change its shape inside `openapi.json`
and a regeneration carries that change to the GUI as a compile error, not as a runtime surprise.

Today the GUI carries a hand-written client at `TucanoTestGUI/src/api/client.ts` (hand-declared interfaces such
as `TestCase`, `TestSuite`, `Project`, `TestRun`, `Milestone`, and hand-built `fetch` calls). That file is the
thing the generated client replaces; the migration is tracked in the GUI repository, not here.

## The contract

`openapi.json` is served at `GET /openapi.json`, rendered by Swagger UI at `GET /api-docs`, and checked into
the repository. The document declares OpenAPI `3.0.3` and 38 paths, which is exactly the router's route set:
`tests/service.rs::openapi_document_matches_the_registered_routes` asserts that the documented paths equal
`api::ROUTES` minus `api::UNDOCUMENTED_ROUTES` (the three served-but-unpublished entries: the `/api-docs/`
trailing-slash alias and the two retired flat creation routes `POST /test_suites` and `POST /test_cases`).
That assertion is what keeps the contract honest about *which routes exist*; it does not, by itself, assert
that each operation's bodies and responses are fully typed (see *Completeness* below).

## Generation

Recommended generator: **`@openapitools/openapi-generator-cli`** with the `typescript-fetch` generator. The
wrapper resolves the generator JAR, so the only prerequisite is a Node/npm toolchain (the GUI already requires
Node).

Exact invocation, run from the API repository root writing into a sibling `TucanoTestGUI` checkout:

```sh
npx --yes @openapitools/openapi-generator-cli generate \
  -i openapi.json \
  -g typescript-fetch \
  -o ../TucanoTestGUI/src/api/generated \
  --additional-properties=useSingleRequestParameter=true
```

Rules for using it:

- **Pin the version.** `TucanoTestGUI` commits an `openapitools.json` that records the generator version, so
  the invocation above resolves to a fixed generator instead of whatever is latest. A generated client that
  changes because the generator changed is not a contract change.
- **Commit the output, check for drift.** The generated tree is committed, and CI regenerates and fails if the
  output differs from what is committed. A regeneration that changes the client is then a reviewable diff, and
  a stale client is a red build rather than a runtime bug.
- **Feed it the shipped document.** Generate from the API's checked-in `openapi.json` (equivalently, from
  `GET /openapi.json` of a running service — they are the same document). Never generate from a hand-edited
  copy: the point of the exercise is that the published document is the only input.
- **Regenerate on contract change.** Any change to `openapi.json` requires regenerating and committing the
  client in the same piece of work, so the two never disagree on `main`.

For a Rust consumer, the same document drives `progenitor`; the choice of generator does not change the
boundary, only the language of the generated types.

## The cross-check

A generated client is only as good as the document it is generated from. The contract tests required by
[`AgentRules/test/contract.md`](../../AgentRules/test/contract.md) are the guard that `openapi.json` matches the
live API:

- `tests/service.rs::openapi_document_matches_the_registered_routes` — the documented path set equals the
  registered route set.
- `tests/service.rs::router_serves_every_declared_route` — every declared route is actually served.
- The per-area suites (`tests/projects.rs`, `tests/suites.rs`, `tests/cases.rs`, `tests/runs.rs`,
  `tests/milestones.rs`, `tests/configurations.rs`, `tests/attachments.rs`, `tests/tags.rs`,
  `tests/validation.rs`) — the status codes, error envelope, and payload shapes the document describes.
- `tests/service.rs` also pins the published error contract and the two non-envelope answers (the `413`
  body-size limit and the multipart `400`).

The typing of bodies and success responses was the original gap; it is now closed (see the table below), so the
cross-check in `tests/service.rs` pins the document's operation metadata, write bodies, success payloads, error
codes, and security posture alongside the route set.

## Completeness of `openapi.json` for generation

The document is complete for the route set (verified against `api::ROUTES`) and for read models: each
single-document `GET` returns a `$ref` into `components.schemas` (`Project`, `TestSuite`, `TestCase`,
`TestRun`, `Milestone`, `TestConfiguration`, `MilestoneProgress`), the identifier-listing `GET`s return arrays
of strings, and the attachment, defect-link, and import operations reference `StepAttachment`, `DefectLink`,
and the `Import*` summary schemas. Every gap recorded when this document was written (verified against `main`
at `b17b26a`) has since been closed; re-verified against the document on 2026-09-11 (68 operations):

| # | Gap (when written) | Affected | Resolution |
| --- | --- | --- | --- |
| G1 | No `operationId` on any operation (all 60) | all operations | **Closed.** All 68 operations carry a stable `operationId`. ([#145](https://github.com/TucanoTechnology/TucanoTestAPI/issues/145)) |
| G2 | No operation `tags` (all 60) and no document-level `tags` array | all operations | **Closed.** All 68 operations carry a resource-family tag and the document declares the tag set (`Service`, `Projects`, `TestSuites`, `TestCases`, `TestRuns`, `Milestones`, `Configurations`, `Reports`, `Auth`). ([#145](https://github.com/TucanoTechnology/TucanoTestAPI/issues/145)) |
| G3 | `requestBody` declared as a bare `{"type": "object"}` with no properties and no `required` | the write operations | **Closed.** Each write operation references a named request schema (`ProjectCreateRequest`, `ProjectUpdateRequest`, `TestRunCreateRequest`, `TestCaseUpdateRequest`, `MilestoneCreateRequest`, `TestConfigurationUpdateRequest`, `Duplicate*Request`, `CompositionRequest`, …), so a generated client constructs and type-checks bodies. The binary and multipart uploads remain body-less, which is correct. ([#140](https://github.com/TucanoTechnology/TucanoTestAPI/issues/140)) |
| G4 | Success responses with no `content` schema | 31 API operations | **Closed.** Every JSON success response carries a `content` schema (create and duplicate answer `{"message", "id"}`, update and delete answer `{"message"}`); the three operational endpoints (`/health`, `/openapi.json`, `/api-docs`) are deliberately non-JSON. ([#140](https://github.com/TucanoTechnology/TucanoTestAPI/issues/140)) |
| G5 | Five duplicate path items are `$ref`s into `#/components/x-duplicate*` extension objects | the five `/duplicate` routes | **Closed.** Every duplicate operation is an inline path item and no `x-` component objects remain. ([#145](https://github.com/TucanoTechnology/TucanoTestAPI/issues/145)) |
| G6 | `servers` is a single hard-coded `http://localhost:3000` with no variables | document | **Closed.** `servers` is `{scheme}://{host}:{port}` with a variable for each and documented defaults. ([#145](https://github.com/TucanoTechnology/TucanoTestAPI/issues/145)) |
| G7 | No `securitySchemes` and no `security` | document | **Closed.** `components.securitySchemes.bearerAuth` is an HTTP `bearer` scheme, the document carries a global `security: [{"bearerAuth": []}]` requirement, and the five caller-free operations carry `security: []`. ([#130](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130)) |
| G8 | Error `code` is a plain `string` in one shared `Error` schema | every error response | **Closed.** `Error.error.code` is an enum of the published codes (`invalid_id`, `invalid_request`, `invalid_status`, `invalid_multipart`, `missing_file`, `not_found`, `conflict`, `storage_error`, plus the auth codes `missing_token`, `invalid_token`, `token_expired`, `invalid_credentials`, `invalid_refresh_token`, and `forbidden`). ([#140](https://github.com/TucanoTechnology/TucanoTestAPI/issues/140)) |

Ticket [#81](https://github.com/TucanoTechnology/TucanoTestAPI/issues/81) closed the renderability defect —
the `x-crud`/`x-resource` `$ref` shortcuts that made Swagger UI show no operations for projects, test suites,
test runs, test cases, milestones, and configurations are gone, so each of those resources' CRUD operations is
now an inline path item and no path-item `$ref`s remain. The schema-level residue #81 did not cover — the typed
write bodies, success responses, and error codes — was closed by [#140](https://github.com/TucanoTechnology/TucanoTestAPI/issues/140);
the operation metadata was closed by [#145](https://github.com/TucanoTechnology/TucanoTestAPI/issues/145); and
the security scheme was published by [#130](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130). The
gaps above are kept as the record of what generation needed and how each was resolved.

The `docs/contracts/api-compatibility.md` "Client generation" section the issue also names is a protected file
on the API (code) track, so that section and its pointer back to this document are deferred there; the
deferral is recorded on [#99](https://github.com/TucanoTechnology/TucanoTestAPI/issues/99).

## Non-goals

- No change to the API surface, `openapi.json`, or any stored document by this issue — it is the strategy
  record.
- No GUI code: the client migration and the drift check live in `TucanoTestGUI`.
- No generator version upgrade policy beyond pinning the version and committing generated output.

## Follow-ups

All three original follow-ups have landed against `openapi.json`; the remaining work is on the GUI track.

1. **Typed write bodies and success responses (G3, G4, G8).** Closed by
   [#140](https://github.com/TucanoTechnology/TucanoTestAPI/issues/140): the write operations name request
   schemas, the success responses carry `content` schemas, and `Error.error.code` is a closed enum.
2. **Operation metadata (G1, G2, G5, G6).** Closed by
   [#145](https://github.com/TucanoTechnology/TucanoTestAPI/issues/145): stable `operationId`s, resource-family
   `tags`, inline duplicate path items, and parameterized `servers`.
3. **Authentication (G7).** Closed by [#130](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130):
   `securitySchemes.bearerAuth` and a global `security` requirement are published, so a regenerated client
   carries credentials and the five caller-free operations stay unauthenticated.

## Files

| File | Change |
| --- | --- |
| `docs/architecture/gui-client-boundary.md` | This strategy record (new). |
| `README.md` | One documentation-table row pointing here. |
| `docs/contracts/api-compatibility.md` | **Deferred to the API track** — a *Client generation* section and a pointer to this file. Recorded on #99. |
