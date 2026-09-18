# Case versioning and history

Every test case carries a `version` number and a `lastModified` stamp, both written by the API. When
you change something the test actually asserts, the API takes a snapshot of the case *before* the
change and bumps the version — so you can answer "what did this case look like at version 3, and
what changed in 4?".

Exact schemas and status codes live in the Swagger UI at `/api-docs` and
[`openapi.json`](../../openapi.json).

## The two managed fields

| Field | Type | Notes |
| --- | --- | --- |
| `version` | integer ≥ 1 | Starts at `1`; a qualifying edit increments it |
| `lastModified` | ISO-8601 UTC string | When the case last changed |

Both are **API-managed**: a value you send for them is not trusted. Send them and they are ignored;
the API stamps its own.

## What counts as a change

Only four fields qualify for versioning. Editing anything else updates the case without producing a
new version:

| Qualifies (bumps the version) | Does not qualify (no bump, no snapshot) |
| --- | --- |
| `title` | `description` |
| `steps` | `priority` |
| `preconditions` | `severity` |
| `expectedResult` | `testType` |
| | `exploratory` |
| | `attachments` |
| | `tags` |

The split is deliberate: the qualifying four are the ones that change *what the test asserts*.
Re-labelling a case or filing an extra screenshot is not a new revision of the test.

## What a version bump does

When a qualifying `PUT` (`updateTestCase`) arrives:

1. The **pre-update** document is written to `revisions/v<current>.json` — snapshot first, never
   after.
2. `version` is incremented.
3. `lastModified` is restamped.
4. The live `test-case.json` is updated.

A `PUT` that touches only non-qualifying fields changes the live document and leaves `version`,
`lastModified` and the `revisions/` folder untouched.

```text
<case folder>/
├── test-case.json          the live document
└── revisions/
    ├── v1.json             the document as it stood at version 1
    └── v2.json             the document as it stood at version 2
```

Snapshots are **immutable**. Nothing writes to `revisions/` after the fact, and there is no route
that edits one.

## Reading the history

| Route | Operation id | Returns |
| --- | --- | --- |
| `GET /test_cases/{id}/history` | `listTestCaseHistory` | Every version, newest first |
| `GET /test_cases/{id}/history/{version}` | `getTestCaseVersion` | One snapshot's document |

The listing answers `CaseHistoryEntry` objects:

| Field | Notes |
| --- | --- |
| `version` | The version number |
| `lastModified` | When that version was created |
| `changedFields` | The qualifying fields that differed between that version and the one that superseded it |

`changedFields` is a **difference**, not a summary of the whole document: it lists the qualifying
fields that changed between the snapshot and the version that replaced it (the newest snapshot is
compared against the live document). A case that has never been re-versioned lists `[]`.

```sh
curl -s http://localhost:3100/test_cases/refund-partial.json/history
```

```json
[
  {"version":2,"lastModified":"2026-09-14T12:04:11Z","changedFields":["steps"]},
  {"version":1,"lastModified":"2026-09-14T11:52:03Z","changedFields":["title","expectedResult"]}
]
```

Fetching a specific version returns the stored document:

```sh
curl -s http://localhost:3100/test_cases/refund-partial.json/history/1
```

Two versions answer `404 not_found` rather than an empty body:

- **A version with no snapshot.** Only superseded versions have a file, so the current version is
  not fetchable this way — read the live case instead.
- **A version that never existed.**

A non-positive `{version}` in the path is `400 invalid_request`.

## Case versions in a run

A run records which version of each case it executed, in `caseVersions`. The first write wins: once
the run has pinned a case's version, a later edit to the case does not change what the run claims it
tested. A case that was never versioned is pinned as `1`. See
[Test runs and results](test-runs-and-results.md).

## Worked example

Starting from a running instance on `http://localhost:3100` (see
[Installation and first project](getting-started.md)), with the `refund-partial.json` case from
[Projects, suites, and cases](projects-suites-and-cases.md) present. Authentication is assumed off;
with it on, add `-H "Authorization: Bearer $TOKEN"`.

**1. A fresh case starts at version 1 with an empty history.**

```sh
curl -s http://localhost:3100/test_cases/refund-partial.json
curl -s http://localhost:3100/test_cases/refund-partial.json/history
```

```json
{"name":"Partial refund returns the difference","testCaseId":"refund-partial.json","version":1,"lastModified":"…"}
```

```json
[]
```

**2. Change something the test asserts.**

```sh
curl -s -X PUT http://localhost:3100/test_cases/refund-partial.json \
  -H 'Content-Type: application/json' \
  -d '{"steps":["Refund half the total","Confirm the credit"]}'
```

**3. Read the history again** — the *pre-update* document was snapshotted as `v1`:

```sh
curl -s http://localhost:3100/test_cases/refund-partial.json/history
```

```json
[{"version":2,"lastModified":"…","changedFields":["steps"]}]
```

```sh
curl -s http://localhost:3100/test_cases/refund-partial.json/history/1
```

`v1.json` is the case as it stood **before** the steps were replaced.

**4. Change a field that does not qualify.**

```sh
curl -s -X PUT http://localhost:3100/test_cases/refund-partial.json \
  -H 'Content-Type: application/json' -d '{"priority":"Critical","tags":["refunds"]}'
```

`version` stays `2`. No new file appears under `revisions/`, and `lastModified` is unchanged — the
live document is the only thing that moved, and reading the history shows the same single entry.

## Things that catch people out

| Symptom | Cause |
| --- | --- |
| `version` did not move after a `PUT` | The `PUT` touched only non-qualifying fields (`priority`, `description`, `tags`, `attachments`, …) |
| `lastModified` did not move after a `PUT` | Same reason — it is restamped only on a qualifying change |
| `404 not_found` fetching a version | The version has no snapshot (it is the live version), or it never existed |
| `changedFields` looks incomplete | It lists only the **qualifying** fields, and only those that differed from the version that superseded it |
| A version I sent was ignored | `version` and `lastModified` are API-managed; the API stamps its own |
| A duplicate reports a `version` but no history | Duplicating a case copies the document only, so the copy inherits the source's `version` while no `revisions/` snapshot sits beside it and `GET /test_cases/{copy}/history` is empty. See [Composing and duplicating](composing-and-duplicating.md) |
| Looking for a revert route | There is none. Snapshots are read-only; to go back, `PUT` the old values back as a new version |

## Next

| I want to… | Read |
| --- | --- |
| Add steps and attachments, the fields that most often bump a version | [Steps and attachments](steps-and-attachments.md) |
| Carry a case's revision history to another parent | [Composing and duplicating](composing-and-duplicating.md) — composition `copy` duplicates the case folder; `duplicate` copies the document only |
| See how a run pins the version it executed | [Test runs and results](test-runs-and-results.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for the `version` and `lastModified` fields,
the history routes and the `CaseHistoryEntry` schema named here; the
[case versioning plan](../contracts/test-case-versioning-plan.md) for the qualifying-field list, the
snapshot rule and the `changedFields` semantics. Where this page and one of those disagree, the
source wins and this page is a bug.*
