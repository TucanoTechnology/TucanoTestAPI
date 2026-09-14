# Composing and duplicating

Two related ideas cover almost every "I want the same tests over there" request:

- **Composition** — put an existing suite or case into a parent it is not already in, choosing
  whether to *copy* it there or *move* it.
- **Duplication** — make a near-identical second project, suite or case with a new identifier.

Neither is a database join: everything is folders, and the API is the only writer. Exact request
schemas and status codes live in the Swagger UI at `/api-docs` and
[`openapi.json`](../../openapi.json).

## `copy` versus `move`

Composition requests accept `"mode": "copy" | "move"` and **default to `copy`**.

| Mode | What happens | The source | Both copies |
| --- | --- | --- | --- |
| `copy` (default) | The entity is **duplicated** into the target parent — duplicate-on-include | Keeps its home, untouched | Editable independently from then on |
| `move` | The entity is **relocated**; the target parent becomes its only physical home | No longer exists there | n/a — there is only one |

> **A `copy` is a deep, independent duplicate.** The copy gets its own folder and its own document.
> Editing the copy does not change the original and vice versa. This is why copy-on-include can
> legitimately produce two folders holding the same case id under different parents — and why
> document-level routes on that id can then answer `409 conflict` until you address the occurrence
> through its parent.

`move` is opt-in because it is the destructive one: the source disappears. Reach for `copy` when you
are unsure.

## The composition routes

| Route | Operation id | What it composes |
| --- | --- | --- |
| `POST /projects/{id}/test_suites` | `addProjectTestSuite` | A suite into a project |
| `POST /projects/{id}/test_cases` | `addProjectTestCase` | A case into a project (directly) |
| `POST /test_suites/{id}/test_cases` | `addTestSuiteCase` | A case into a suite |

All three take the same body shape, `CompositionRequest`, which is a **creation-or-placement**
request. The grammar of the body decides which one you are doing:

| You are… | Send | Notes |
| --- | --- | --- |
| **Creating** a suite | `{"name": "…"}` | The suite id is always `<name>.json` |
| **Creating** a case | `{"testCaseId": "…", "title": "…", "expectedResult": "…"}` | `expectedResult` is required alongside `title` |
| **Placing** an existing suite | `{"suiteId": "…", "mode": "copy"\|"move"}` | `suiteId` names the suite to bring here |
| **Placing** an existing case | `{"testCaseId": "…", "mode": "copy"\|"move"}` | `testCaseId` here names the case to bring; `title`/`expectedResult` are absent |

Rules the API enforces:

- **`mode` alongside the fields that create is `400 invalid_request`.** A `mode` on a body that is
  creating something has nothing to act on, so it is refused rather than silently ignored.
- **A `mode` other than `copy` or `move` is `400 invalid_request`.**
- **Fields the reading in use does not know are ignored**, so `CompositionRequest` does not claim
  `additionalProperties: false`.

A successful create or placement answers `201` with `{"message": …, "id": …}`
(`CompositionResponse`), where `id` is the identifier of the new child.

## Worked example: copy a case into a second suite

Starting from a running instance on `http://localhost:3100` (see
[Installation and first project](getting-started.md)). Authentication is assumed off; with it on,
add `-H "Authorization: Bearer $TOKEN"`.

**1. Set up two suites and a case.**

```sh
curl -s -X POST http://localhost:3100/projects \
  -H 'Content-Type: application/json' -d '{"name":"Payments"}'

curl -s -X POST http://localhost:3100/projects/Payments.json/test_suites \
  -H 'Content-Type: application/json' -d '{"name":"Refunds"}'

curl -s -X POST http://localhost:3100/projects/Payments.json/test_suites \
  -H 'Content-Type: application/json' -d '{"name":"Regression"}'

curl -s -X POST http://localhost:3100/test_suites/Refunds.json/test_cases \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"refund-partial.json","title":"Partial refund returns the difference","expectedResult":"The card is credited the refunded amount"}'
```

**2. Copy that case into `Regression`.** Omit `mode` to take the default:

```sh
curl -s -X POST http://localhost:3100/test_suites/Regression.json/test_cases \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"refund-partial.json","mode":"copy"}'
```

```json
{"id":"refund-partial.json","message":"Test case created"}
```

Both suites now hold a case with that id, in **separate folders**:

```text
projects/Payments.json/
├── project.json
├── Refunds.json/
│   ├── suite.json
│   └── refund-partial.json/test-case.json
└── Regression.json/
    ├── suite.json
    └── refund-partial.json/test-case.json
```

The id now resolves ambiguously, and the API says so rather than guessing:

```sh
curl -si http://localhost:3100/test_cases/refund-partial.json
```

```
HTTP/1.1 409 Conflict

{"error":{"code":"conflict","message":"…","requestId":"…"}}
```

Reading each occurrence through its parent works because each parent-scoped route is unambiguous:

```sh
curl -s http://localhost:3100/test_suites/Refunds.json/test_cases
curl -s http://localhost:3100/test_suites/Regression.json/test_cases
```

Edit one of them and the other is untouched — that is the point of duplicate-on-include:

```sh
curl -s -X PUT http://localhost:3100/test_cases/refund-partial.json \
  -H 'Content-Type: application/json' -d '{"priority":"Critical"}'
```

`PUT` on the ambiguous id answers `409` as well. To edit a specific occurrence, first make the id
unique again — delete or move the other copy, or address the case through a parent-scoped route
where one exists.

**3. Move instead of copy.** With `mode` set to `move`, the case leaves its current home:

```sh
curl -s -X POST http://localhost:3100/projects/Payments.json/test_cases \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"refund-partial.json","mode":"move"}'
```

```json
{"id":"refund-partial.json","message":"Test case created"}
```

The case folder now exists **only** at `projects/Payments.json/refund-partial.json/`, and the two
suite copies are gone. The id is no longer ambiguous.

**4. Remove a child from one parent without deleting it everywhere.**

```sh
curl -s -X DELETE http://localhost:3100/test_suites/Regression.json/test_cases/refund-partial.json
```

`removeTestSuiteCase` removes the occurrence under that suite. Use it — never the document-level
`DELETE /test_cases/{id}` — when the id still resolves to several folders and you only want one of
them gone.

## Duplication routes

Duplication makes a new entity from an existing one, in place.

| Route | Operation id | Body |
| --- | --- | --- |
| `POST /projects/{id}/duplicate` | `duplicateProject` | `DuplicateRequest` |
| `POST /test_suites/{id}/duplicate` | `duplicateTestSuite` | `DuplicateRequest` |
| `POST /test_cases/{id}/duplicate` | `duplicateTestCase` | `DuplicateCaseRequest` |
| `POST /milestones/{id}/duplicate` | `duplicateMilestone` | `DuplicateRequest` |
| `POST /test_runs/{id}/duplicate` | `duplicateTestRun` | `DuplicateRunRequest` |

| Body field | Applies to | Meaning |
| --- | --- | --- |
| `newId` | all | Identifier for the copy; derived from the source when omitted |
| `newName` | projects, suites, milestones | Name for the copy; the source name is kept when omitted |
| `newTitle` | cases | Title for the copy; the source title is kept when omitted |

A duplicate answers `201` with `{"message": …, "id": …}` (`CreateResponse`), the id being the copy's.

```sh
curl -s -X POST http://localhost:3100/test_cases/refund-partial.json/duplicate \
  -H 'Content-Type: application/json' \
  -d '{"newId":"refund-partial-copy.json","newTitle":"Partial refund (variant)"}'
```

```json
{"id":"refund-partial-copy.json","message":"Test case duplicated"}
```

What comes along matters:

- **Duplicating a project or a suite duplicates the whole subtree.** Suites keep their cases; cases
  keep their steps, their attachments on disk, and their `revisions/` snapshots. The copy therefore
  starts with the source's history, and its own future edits add to it.
- **Duplicating a case copies the case folder**, including its attachments, its `steps/<n>/`
  directories and its revision snapshots.
- **Runs and milestones are snapshots, not containers.** Duplicating a milestone copies which suites
  and runs it references, and those references still point at the same runs — nothing is re-executed
  or re-snapshotted. Duplicating a run copies its recorded snapshot verbatim. See
  [Milestones](milestones.md) and [Test runs and results](test-runs-and-results.md).

Because a duplicate's copy is independent from that point on, a `duplicate` is the right tool for
branching a suite into a new variation without touching the original — where composition `copy` is
the tool for placing one existing case into an additional, already-existing parent.

## Runs always copy

There is no `mode` for a run. `POST /test_runs/{id}/test_suites` (`addTestRunTestSuite`) and
`POST /test_runs/{id}/test_cases` (`addTestRunTestCase`) take `{"suiteId": …}` and
`{"testCaseId": …}` respectively and **always snapshot**: the run embeds a full copy of each
document it includes and never owns or re-reads the live one. A later edit to the source case
therefore leaves the run exactly as recorded. See [Test runs and results](test-runs-and-results.md).

## Things that catch people out

| Symptom | Cause |
| --- | --- |
| `400 invalid_request` | `mode` was sent alongside `name`/`title`, or was neither `copy` nor `move` |
| `400 invalid_request` | A case creation body carried `title` but no `expectedResult` |
| `409 conflict` | A document-level route on an id that copy-on-include placed under several parents — address it through its parent |
| `404 not_found` | The parent, the suite or the case named in the body does not exist |
| The source vanished | `mode` was `move`, or the default was overridden somewhere upstream. Check who sent the request |
| A `POST …/test_cases` placed the wrong thing | The body created instead of placing, or vice versa: the presence of `title`/`expectedResult` makes it a create, of `testCaseId` alone makes it a placement |
| Deleting the copy deleted the original | It did not — but check which occurrence the id resolved to, because a document-level delete on an ambiguous id is refused rather than applied to one of them |

## Next

| I want to… | Read |
| --- | --- |
| See the folder layout these operations produce | [Projects, suites, and cases](projects-suites-and-cases.md) |
| Understand what a copy does to revision history | [Case versioning and history](case-versioning-and-history.md) |
| Record a run against a snapshot of the cases | [Test runs and results](test-runs-and-results.md) |
| Duplicate a milestone or read its progress | [Milestones](milestones.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for `CompositionRequest`, the duplicate
request bodies, the response schemas and every operation id named here; the storage concept in the
[repository README](../../README.md#storage-concept) for the copy-by-default rule; the
[compatibility contract](../contracts/api-compatibility.md) for the copy/move-include semantics
plans (#23, #67) and real-home storage (#65). Where this page and one of those disagree, the source
wins and this page is a bug.*
