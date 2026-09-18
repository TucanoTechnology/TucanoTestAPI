# Milestones

A **milestone** is a dated goal that references the suites and runs that are meant to get you there.
It is a pointer document, not a container: it holds no cases of its own, and its progress is derived
from the results recorded in the runs it references — never from the live case documents.

Exact schemas and status codes live in the Swagger UI at `/api-docs` and
[`openapi.json`](../../openapi.json).

## What a milestone is

A flat document, inside the project that owns it, at
`projects/<project>/milestones/<milestoneId>.json`:

| Field | Notes |
| --- | --- |
| `milestoneId` | **Required.** The identifier; derived as `<name>.json` when omitted |
| `name` | **Required.** The displayed name |
| `description` | Free prose |
| `startDate`, `targetDate` | The window, as dates |
| `status` | Free-form lifecycle label |
| `testSuiteIds` | Identifiers of the suites in scope |
| `testRunIds` | Identifiers of the runs in scope |

A milestone has a **home project** — the project it is created in, whose `milestones/` folder holds
the document. Its id is unique there, so two projects may each hold a `v1.0.json`; the document
routes under `/milestones/{id}` answer `409 conflict` when they cannot tell which one you mean, and
name the parent-scoped route to use instead. See
[Storage layout v3](../architecture/adr-storage-layout-v3.md) for the decision.

| Route | Operation id | What it does |
| --- | --- | --- |
| `GET /projects/{id}/milestones` | `listProjectMilestones` | Lists one project's milestones, as a sorted array of ids |
| `POST /projects/{id}/milestones` | `addProjectMilestone` | Creates the milestone inside the project |
| `DELETE /projects/{id}/milestones/{milestone_id}` | `removeProjectMilestone` | Removes the milestone from the project |
| `GET /milestones` | `listMilestones` | Lists them across every project; supports `?filter=` only |
| `GET /milestones/{id}` | `getMilestone` | Reads one |
| `PUT /milestones/{id}` | `updateMilestone` | Partial update |
| `DELETE /milestones/{id}` | `deleteMilestone` | Removes one |
| `POST /milestones/{id}/duplicate` | `duplicateMilestone` | Copies it, references included |
| `GET /milestones/{id}/progress` | `getMilestoneProgress` | Derives progress from the referenced runs |

Creation is project-scoped: `POST /projects/{id}/milestones` creates the milestone in that project
and answers `201` with `{"message": "Milestone created", "id": …}`; an unknown project answers
`404 not_found` and an id already taken in that project answers `409 conflict`. The flat
`POST /milestones` is retired and answers `400 invalid_request` naming the replacement. The bare
`GET /milestones` stays served for compatibility but is deliberately absent from `openapi.json`.

## How a milestone resolves what it references

A milestone resolves each `testSuiteIds` and `testRunIds` entry through its own project first, so a
run id another project happens to use as well still names the intended run. That is a resolution
rule, not a creation constraint: **a milestone with no references at all is legal.** It is a
planning milestone for work not yet scoped, and its progress reports zeros until you point it at
something.

```sh
curl -s -X POST http://localhost:3100/projects/Payments.json/milestones \
  -H 'Content-Type: application/json' \
  -d '{"milestoneId":"release-4.2.json","name":"Release 4.2",
       "targetDate":"2026-10-01","testSuiteIds":["Refunds.json"],"testRunIds":["run-2026-09-14.json"]}'
```

```json
{"id":"release-4.2.json","message":"Milestone created"}
```

Writing a milestone requires `owner` in its **home project** and in every project its references
reach; reading it requires `viewer` in the same set. Creating one therefore needs `owner` in the
project that will hold it, and a milestone that references nothing is governed by its home alone.
See the [authentication section](../../README.md#authentication) of the README for the role model.

## Progress is derived, not stored

`GET /milestones/{id}/progress` (`getMilestoneProgress`) answers a `MilestoneProgress` computed from
the referenced runs. One run's **population** is every case it holds: the cases its `testCases`
snapshot pins, the cases the suites it embedded brought with them, and the cases it has a recorded
result for. Progress counts that union once per case id, so a case both pinned and recorded counts
once, and the five buckets **partition** `totalCases` — `passed + failed + blocked + untested +
retest` always equals `totalCases`. A case the run holds without a result is `untested`.

```sh
curl -s http://localhost:3100/milestones/release-4.2.json/progress
```

```json
{
  "milestoneId": "release-4.2.json",
  "totalCases": 2,
  "passed": 1,
  "failed": 1,
  "blocked": 0,
  "untested": 0,
  "retest": 0,
  "passPercentage": 50.0
}
```

> **Nothing about this number is live.** A milestone reports what its runs recorded, at the moment
> those runs recorded it. Editing a case today does not move a milestone, because the run it points
> at still holds the result it held. To change the number, record a new result or run.

The runs it reads are the ones named in `testRunIds`, each looked up in the milestone's own project
first. A reference no project holds is skipped and the numbers recompute over the runs that remain;
one that two or more projects hold outside the home answers `409 conflict` rather than picking
arbitrarily, because an arbitrary pick would report a wrong number.

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

**1. Create the milestone in the project, over a suite and a run.**

```sh
curl -s -X POST http://localhost:3100/projects/Payments.json/milestones \
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
{"milestoneId":"release-4.2.json","totalCases":2,"passed":1,"failed":1,"blocked":0,"untested":0,"retest":0,"passPercentage":50.0}
```

The run holds two cases. `refund-partial.json` reaches it twice — the `Refunds.json` suite snapshot
brought it in, and a recorded result names it — while `smoke-checkout.json` reaches it through its
result alone. Neither counts twice, so `totalCases` is 2 and the buckets sum to it.

**3. Improve a result and see progress move** — not because the milestone changed, but because its
run recorded something new:

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/results \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"refund-partial.json","status":"Passed","notes":"Fixed by PAY-412"}'

curl -s http://localhost:3100/milestones/release-4.2.json/progress
```

```json
{"milestoneId":"release-4.2.json","totalCases":2,"passed":2,"failed":0,"blocked":0,"untested":0,"retest":0,"passPercentage":100.0}
```

`passPercentage` moves because the run moved, and only for that reason. The population is unchanged —
the same two cases — and only one bucket gave a case up to another.

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
| Progress never changes | The progress comes from the referenced runs' recorded results. Editing cases does nothing; record a result or add a run |
| `totalCases` is larger than the cases the run's `testCases` array shows | A run also holds the cases its embedded suite snapshots brought and the cases it has a result for; progress counts that whole union once per case id |
| A duplicate has the same numbers as the original | Duplication copies the references; the runs are shared, not re-executed |
| `404 not_found` on a milestone id | The `milestoneId` is the file name (`<name>.json`), not the display `name` |
| `?tags=` on `GET /milestones` does nothing | Milestones carry no `tags` field, so the listing offers only `?filter=` |
| `400 invalid_request` on `POST /milestones` | The flat creation route is retired: create the milestone inside its project with `POST /projects/{id}/milestones` |
| `404 not_found` on `POST /projects/{id}/milestones` | No project has that `id` — a milestone is created inside a project that exists |
| `409 conflict` on `GET /milestones/{id}` | Two or more projects hold a milestone with that id; name the home with `GET`/`PUT`/`DELETE /projects/{id}/milestones/{milestone_id}` |
| `409 conflict` on `GET /milestones/{id}/progress` | A run the milestone references exists in two or more projects outside its home, so there is no single answer to report |
| `403 forbidden` | Writing a milestone requires `owner` in its home project and in every project its references reach |

## Next

| I want to… | Read |
| --- | --- |
| See how runs record the results a milestone aggregates | [Test runs and results](test-runs-and-results.md) |
| Understand the summary report's fields and filters | [Result imports and reports](imports-and-reports.md) |
| Duplicate a milestone alongside other entities | [Composing and duplicating](composing-and-duplicating.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for the milestone routes and the
`MilestoneProgress` schema named here; the storage concept in
the [repository README](../../README.md#storage-concept) for milestones referencing suites and runs
and deriving progress from their results; [`docs/architecture/adr-storage-layout-v3.md`](../architecture/adr-storage-layout-v3.md)
for why milestones live inside their project, that a reference-less milestone is legal, and how a
shared identifier resolves; the
[compatibility contract](../contracts/api-compatibility.md) for the milestone duplicate plan (#68) and
the progress population rule (#286).
Where this page and one of those disagree, the source wins and this page is a bug.*
