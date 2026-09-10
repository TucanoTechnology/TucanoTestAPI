# Test Case Versioning Plan (Issue #89)

Issue: [#89](https://github.com/TucanoTechnology/TucanoTestAPI/issues/89) — the design step split from
[#52](https://github.com/TucanoTechnology/TucanoTestAPI/issues/52) (which folded in the duplicate
[#38](https://github.com/TucanoTechnology/TucanoTestAPI/issues/38)). This plan records field names, snapshot
shape, trigger rules, and addressing for test-case versioning and revision audit history. It is documentation
only: no code, no schema, and no stored document changes with this issue.

`docs/contracts/api-compatibility.md` remains the compatibility authority and the place a plan of record is
expected to live. It is a protected file on the API (code) track, so the *Test case versioning* section inside
it and its pointer back to this file are deferred to that track; the deferral is recorded on
[#89](https://github.com/TucanoTechnology/TucanoTestAPI/issues/89). Until that section lands, **this file is the
recorded plan**.

## Field additions (the breaking-change caveat)

The legacy Draft 2020-12 schemas set `additionalProperties: false`, so adding any field is a breaking change and
requires an explicit versioning plan before implementation — this document is that plan for the two fields below.

Two optional fields are added to **`TestCase` only**, in the model's camelCase wire shape:

| Field | Type | Format | Notes |
| --- | --- | --- | --- |
| `version` | `Option<u64>` | integer, starting at `1` | The case's current revision number. |
| `lastModified` | `Option<String>` | ISO-8601 UTC | When the current version was written. |

Both carry `#[serde(skip_serializing_if = "Option::is_none")]`, so:

- A document persisted before this change — with neither field present — still deserializes (`version: None`,
  `lastModified: None`); `additionalProperties: false` stays satisfied because the field is declared.
- Both fields are omitted from the wire whenever they are `None`, so a legacy consumer sees no new key on a
  document that has never been versioned.
- Stored documents are never rewritten by a read, so a pre-existing case keeps its on-disk shape until a
  qualifying update (below) writes a new version.

Creation records `version: 1` and a `lastModified` timestamp, so a case created after this change carries both
fields from the start. A legacy consumer that rejects unknown fields will therefore refuse such a document —
the same wire-addition caveat the compatibility contract records for other optional fields.

No field is added to `TestSuite`, `Project`, `TestRun`, or `TestCaseResult` by this plan. The run-side capture
of the case version is a sibling child of #52 (see *Run immutability*).

## Revision trigger (the qualifying fields)

A revision is recorded on every update (`PUT /test_cases/{id}`) that changes any of the **qualifying fields**:

- `title`
- `steps`
- `preconditions`
- `expectedResult`

When at least one qualifying field changed, the service:

1. writes an immutable snapshot of the **pre-update** document (`revisions/v{current}.json`), then
2. increments `version` and refreshes `lastModified` on the live document.

A `PUT` that leaves every qualifying field unchanged — edits to `description`, `priority`, `severity`,
`testType`, `exploratory`, `attachments`, or `tags` only — updates the document **without** bumping `version`,
writing no snapshot, and leaving `lastModified` alone. The set of qualifying fields is deliberately narrow so
audit history tracks the executable content of a case, not incidental metadata churn.

## Snapshot layout

Snapshots live inside the case's folder, beside the live document:

```text
<case folder>/
  test-case.json          the live document (current version)
  revisions/
    v1.json               full pre-update document for version 1
    v2.json               full pre-update document for version 2
    ...
  <attachment files>
```

- Each `revisions/v{version}.json` is the full case document as it stood at that version, stored verbatim in the
  legacy shape (including the `version`/`lastModified` it carried).
- Snapshots are **immutable**: a later qualifying update adds a new file and never rewrites an existing one.
- The case's `revisions/` directory is part of the case folder, so the storage semantics already defined for the
  real-home tree apply unchanged: a folder `copy` on include duplicates the revisions with the case, a `move`
  relocates them, and a case `delete` removes them with the folder (Issues #65, #67). A copied case therefore
  starts its own history carrying the source's snapshots.

## Addressing

Two read routes address the history, following the global-dereference rule of the real-home tree (Issue #65):
the identifier resolves to exactly one case folder → operate; no occurrence → `404 Not Found`; several
occurrences → `409 Conflict` naming the parent-scoped routes; an unusable identifier → `400 Bad Request`
(`invalid_id`).

- `GET /test_cases/{id}/history` — lists the recorded revisions, oldest first, as
  `[{ "version", "lastModified", "changedFields" }]`.
  - `version` / `lastModified` are those of the snapshot.
  - `changedFields` is the subset of the qualifying fields (`title`, `steps`, `preconditions`, `expectedResult`)
    that differed between that snapshot and the version that superseded it.
  - The list reflects the snapshots present under `revisions/`; the current version is the live document read
    from the case root, not a snapshot. A case that has never had a qualifying update has no snapshots and lists
    as `[]`.
- `GET /test_cases/{id}/history/{version}` — returns the full snapshot document for that version. A version with
  no snapshot answers `404 Not Found`; so does the current (live) version, which is read through
  `GET /test_cases/{id}`.

Both routes, their parameters, and their response schemas must be represented in `openapi.json` and reachable
through Swagger UI when the code lands (the repository's "not in `openapi.json`, does not exist" rule), and the
`tests/service.rs` route-coverage assertion must be extended with the two paths.

Revert-to-version is **not** part of this plan. If a later issue adds it, the rollback semantics must stay
explicit: a revert creates a **new** revision and never rewrites history.

## Run immutability

Test runs already copy at inclusion and never re-resolve live case documents (Issues #24, #67), so a run stored
before a case edit stays exactly as recorded. The remaining requirement from #38 — a run result linking the
*specific* case version it executed — is a sibling child of #52, not this issue. This plan fixes the addressing
(`version` on the case, immutable `revisions/` snapshots) that the run-side capture references; the run's own
field is out of scope here.

## Files

| File | Change |
| --- | --- |
| `docs/contracts/test-case-versioning-plan.md` | This plan (new). |
| `README.md` | One documentation-table row pointing here. |
| `docs/contracts/api-compatibility.md` | **Deferred to the API track** — a *Test case versioning* section and a pointer to this file. Recorded on #89. |
