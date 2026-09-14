# Milestones

A **milestone** is a dated goal that references the suites and runs that are meant to get you there.
It is a pointer document, not a container: it holds no cases of its own, and its progress is derived
from the results recorded in the runs it references — never from the live case documents.

Exact schemas and status codes live in the Swagger UI at `/api-docs` and
[`openapi.json`](../../openapi.json).

## What a milestone is

A flat document, `milestones/<milestoneId>.json`:

| Field | Notes |
| --- | --- |
| `milestoneId` | **Required.** The identifier; derived as `<name>.json` when omitted |
| `name` | **Required.** The displayed name |
| `description` | Free prose |
| `startDate`, `targetDate` | The window, as dates |
| `status` | Free-form lifecycle label |
| `testSuiteIds` | Identifiers of the suites in scope |
| `testRunIds` | Identifiers of the runs in scope |

| Route | Operation id | What it does |
| --- | --- | --- |
| `GET /milestones` | `listMilestones` | Lists them; supports `?filter=` only |
| `POST /milestones` | `createMilestone` | Creates one from a `MilestoneCreateRequest` |
| `GET /milestones/{id}` | `getMilestone` | Reads one |
| `PUT /milestones/{id}` | `updateMilestone` | Partial update |
| `DELETE /milestones/{id}` | `deleteMilestone` | Removes one |
| `POST /milestones/{id}/duplicate` | `duplicateMilestone` | Copies it, references included |
| `GET /milestones/{id}/progress` | `getMilestoneProgress` | Derives progress from the referenced runs |

## A milestone must point at something

A milestone that references no project is meaningless, so creation requires **at least one**
reference that resolves to a project, through either `testSuiteIds` or `testRunIds`. A create with
neither is refused as `400 invalid_request`.

```sh
curl -s -X POST http://localhost:3100/milestones \
  -H 'Content-Type: application/json' \
  -d '{"milestoneId":"release-4.2.json","name":"Release 4.2",
       "targetDate":"2026-10-01","testSuiteIds":["Refunds.json"],"testRunIds":["run-2026-09-14.json"]}'
```

```json
{"id":"release-4.2.json","message":"Milestone created"}
```

Writing a milestone requires `owner` in every project its references reach; reading it requires
`viewer`. See the [authentication section](../../README.md#authentication) of the README for the
role model.

## Progress is derived, not stored

`GET /milestones/{id}/progress` (`getMilestoneProgress`) answers a `MilestoneProgress` computed from
the **results recorded in the referenced runs**:

```sh
curl -s http://localhost:3100/milestones/release-4.2.json/progress
```

```json
{
  "milestoneId": "release-4.2.json",
  "totalCases": 3,
  "passed": 1,
  "failed": 1,
  "blocked": 0,
  "untested": 1,
  "retest": 0,
  "passPercentage": 33.33
}
```

> **Nothing about this number is live.** A milestone reports what its runs recorded, at the moment
> those runs recorded it. Editing a case today does not move a milestone, because the run it points
> at still holds the result it held. To change the number, record a new result or run.

The same consequence applies to duplication: `POST /milestones/{id}/duplicate` copies which suites
and runs the milestone references, and those references still point at the **same** runs. Nothing is
re-executed and no new snapshot is taken, so a duplicate starts with the same progress until you
point it somewhere else. See [Composing and duplicating](composing-and-duplicating.md).

## Milestones and reporting

The milestone is also a scope for the summary report: `GET /reports/summary?milestoneId=…`
(`getSummaryReport`) aggregates the results reachable through the milestone's referenced runs. That
is the natural way to ask "how is 4.2 doing?" without inventing a second aggregation.

See [Result imports and reports](imports-and-reports.md) for what each field of that report means.

## Worked example

Starting from a running instance on `http://localhost:3100` (see
[Installation and first project](getting-started.md)), with the `Payments` project and the
`run-2026-09-14.json` run from [Test runs and results](test-runs-and-results.md) present.
Authentication is assumed off; with it on, add `-H "Authorization: Bearer $TOKEN"`.

**1. Create the milestone over a suite and a run.**

```sh
curl -s -X POST http://localhost:3100/milestones \
  -H 'Content-Type: application/json' \
  -d '{"milestoneId":"release-4.2.json","name":"Release 4.2","status":"In progress",
       "startDate":"2026-09-14","targetDate":"2026-10-01",
       "testSuiteIds":["Refunds.json"],"testRunIds":["run-2026-09-14.json"]}'
```

**2. Read its progress.**

```sh
curl -s http://localhost:3100/milestones/release-4.2.json/progress
```

```json
{"milestoneId":"release-4.2.json","totalCases":2,"passed":0,"failed":2,"blocked":0,"untested":0,"retest":0,"passPercentage":0}
```

**3. Improve a result and see progress move** — not because the milestone changed, but because its
run recorded something new:

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/results \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"refund-partial.json","status":"Passed","notes":"Fixed by PAY-412"}'

curl -s http://localhost:3100/milestones/release-4.2.json/progress
```

`passPercentage` moves because the run moved, and only for that reason.

**4. Duplicate it for the next release.**

```sh
curl -s -X POST http://localhost:3100/milestones/release-4.2.json/duplicate \
  -H 'Content-Type: application/json' \
  -d '{"newId":"release-4.3.json","newName":"Release 4.3"}'
```

```json
{"id":"release-4.3.json","message":"Milestone duplicated"}
```

**5. Scope the summary report to it.**

```sh
curl -s 'http://localhost:3100/reports/summary?milestoneId=release-4.2.json'
```

## Things that catch people out

| Symptom | Cause |
| --- | --- |
| `400 invalid_request` creating a milestone | Neither `testSuiteIds` nor `testRunIds` referenced a project |
| Progress never changes | The progress comes from the referenced runs' recorded results. Editing cases does nothing; record a result or add a run |
| A duplicate has the same numbers as the original | Duplication copies the references; the runs are shared, not re-executed |
| `404 not_found` on a milestone id | The `milestoneId` is the file name (`<name>.json`), not the display `name` |
| `?tags=` on `GET /milestones` does nothing | Milestones carry no `tags` field, so the listing offers only `?filter=` |
| `403 forbidden` | Writing a milestone requires `owner` in every project its references reach |

## Next

| I want to… | Read |
| --- | --- |
| See how runs record the results a milestone aggregates | [Test runs and results](test-runs-and-results.md) |
| Understand the summary report's fields and filters | [Result imports and reports](imports-and-reports.md) |
| Duplicate a milestone alongside other entities | [Composing and duplicating](composing-and-duplicating.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for the milestone routes, the
`MilestoneCreateRequest` rule and the `MilestoneProgress` schema named here; the storage concept in
the [repository README](../../README.md#storage-concept) for milestones referencing suites and runs
and deriving progress from their results; the
[compatibility contract](../contracts/api-compatibility.md) for the milestone duplicate plan (#68).
Where this page and one of those disagree, the source wins and this page is a bug.*
