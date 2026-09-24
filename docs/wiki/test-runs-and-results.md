# Test runs and results

A **test run** is a point-in-time execution snapshot. You create a run, tell it which suites and
cases are under test, record each case's outcome, and link the defects you found. The run embeds a
copy of everything it executed, so editing a case afterwards never rewrites history: the same case
can appear in several runs with different results, and each run keeps what it saw.

Exact schemas and status codes live in the Swagger UI at `/api-docs` and
[`openapi.json`](../../openapi.json).

## What a run is

Not a folder — a single flat document, inside the project that owns it, at
`projects/<project>/test_runs/<testRunId>.json`:

| Field | Notes |
| --- | --- |
| `testRunId` | **Required.** The identifier; derived as `<name>.json` when omitted |
| `timestamp` | The run's own time. Stamped with the current Unix seconds when omitted |
| `name` | Displayed name |
| `projects` | Identifiers of the projects the run covers |
| `testSuites` | The suites included, as embedded copies |
| `testCases` | The cases included, as embedded copies |
| `results` | One `TestCaseResult` per recorded outcome |
| `tags` | See [Tags and configurations](tags-and-configurations.md) |
| `configurations` | Linked environment identifiers |
| `caseVersions` | Map of case id → the case version this run pinned |

> **A run never owns a case.** Inclusion embeds a copy; the live case stays where it is. This is why
> a run is a snapshot and why deleting the source case leaves the run intact.

A run also has a **home project**: the project it is created in, whose `test_runs/` folder holds the
document. The `projects` array is the coverage list — the projects whose suites and cases the run
executed — and it need not contain the home. A run id is unique within its home project, so two
projects may each hold a `run-2026-09-14.json`; the document routes under `/test_runs/{id}` answer
`409 conflict` when they cannot tell which one you mean, and name the parent-scoped route to use
instead. See [Storage layout v3](../architecture/adr-storage-layout-v3.md) for the decision.

## Creating a run and filling it

| Route | Operation id | What it does |
| --- | --- | --- |
| `GET /projects/{id}/test_runs` | `listProjectTestRuns` | Lists one project's runs, as a sorted array of ids; supports `?filter=`, `?tags=` and `?configuration=` |
| `POST /projects/{id}/test_runs` | `addProjectTestRun` | Creates the run inside the project, from a `TestRunCreateRequest` |
| `DELETE /projects/{id}/test_runs/{run_id}` | `removeProjectTestRun` | Removes the run from the project |
| `GET /test_runs` | `listTestRuns` | Lists runs across every project; supports `?filter=`, `?tags=`, `?configuration=` |
| `GET /test_runs/{id}` | `getTestRun` | Reads one |
| `PUT /test_runs/{id}` | `updateTestRun` | Partial update |
| `DELETE /test_runs/{id}` | `deleteTestRun` | Removes one |
| `POST /test_runs/{id}/test_suites` | `addTestRunTestSuite` | Includes a suite, by `{"suiteId": …}` |
| `POST /test_runs/{id}/test_cases` | `addTestRunTestCase` | Includes a case, by `{"testCaseId": …}` |

Creation is project-scoped: `POST /projects/{id}/test_runs` creates the run in that project and
answers `201` with `{"message": "Test run created", "id": …}`; an unknown project answers
`404 not_found` and an id already taken in that project answers `409 conflict`. The flat
`POST /test_runs` is retired and answers `400 invalid_request` naming the replacement. The bare
`GET /test_runs` stays served for compatibility but is deliberately absent from `openapi.json`.

There is **no `mode`** on the two inclusion routes — or on the creation route: a run always copies.
See [Composing and duplicating](composing-and-duplicating.md).

```sh
curl -s -X POST http://localhost:3100/projects/Payments.json/test_runs \
  -H 'Content-Type: application/json' \
  -d '{"testRunId":"run-2026-09-14.json","name":"Release 4.2 regression","projects":["Payments.json"]}'
```

Omit the identifier and it is derived from the name; omit `timestamp` and the API stamps one.

## Recording a result

`POST /test_runs/{id}/results` (`recordTestRunResult`) takes a `TestResultRequest` and records one
`TestCaseResult` for a case the run holds:

| Field | Notes |
| --- | --- |
| `testCaseId` | **Required.** The case this outcome belongs to |
| `status` | **Required.** `Passed`, `Failed`, `Blocked`, `Untested` or `Retest` |
| `timestamp` | Replaced on every write; stamped with the current Unix seconds when omitted |
| `notes` | Free prose — the "why". Replaced when supplied, cleared by an explicit `null` |
| `durationMs` | Non-negative integer milliseconds. Replaced when supplied, cleared by an explicit `null` |

`attachments` and `defectLinks` are **not** accepted in the body. A `TestCaseResult` carries them on
reads, but the request schema is closed, so sending either is an unknown field and answers
`400 invalid_request`. Link defects through the defect routes below.

The body is validated, not read field by field: an unknown field, a missing `testCaseId` or
`status`, a status outside the list, or a `durationMs` that is not a non-negative integer is
rejected. A `timestamp` must be a non-empty string, or omitted. The route may only update a case the
run already **holds** — one it declared in its `testCases`, carries inside one of its `testSuites`,
or already records a result for. A result for any other case answers `404 not_found`
(`"Test case not in test run"`). To run a new case, include it first with
`POST /test_runs/{id}/test_cases`, or import the report that ran it (see
[Result imports and reports](imports-and-reports.md)).

It answers `200` with the message envelope, not the result:

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/results \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"refund-partial.json","status":"Failed","notes":"Credited the full amount",
       "durationMs":4200}'
```

```json
{"message":"Test result recorded"}
```

Recording a result for a case the run already holds **merges** into that record rather than
replacing it: `status` and `timestamp` are always written, while `notes` and `durationMs` keep their
stored value when the body leaves them out and are replaced when it supplies them. An explicit
`null` clears a field. The run still holds one outcome per case — a second call for the same case
updates the first rather than adding a second record.

## Case versions in a run

A run records which version of each case it executed, in `caseVersions` (case id → version). The
first write wins: once a case's version is pinned in a run, a later edit to the case does not
change what the run says it tested. A case that was never versioned is pinned as `1`. See
[Case versioning and history](case-versioning-and-history.md).

## Defect links

A result can carry defect links — pointers to the issue in your tracker, not copies of it.

| Route | Operation id | What it does |
| --- | --- | --- |
| `POST /test_runs/{id}/results/{case_id}/defects` | `linkResultDefect` | Links a defect |
| `GET /test_runs/{id}/results/{case_id}/defects` | `listResultDefects` | Lists the links |
| `DELETE /test_runs/{id}/results/{case_id}/defects/{link_id}` | `unlinkResultDefect` | Removes one |

The request body is a `DefectLinkRequest`:

| Field | Notes |
| --- | --- |
| `defectId` | **Required.** The tracker's key for the issue |
| `defectUrl` | **Required.** Validated against the tracker's URL shape (below) |
| `trackerType` | **Required.** `jira`, `github`, `gitlab` or `custom` |
| `title`, `status` | Optional copies of the issue's own fields |

| `trackerType` | `defectUrl` must look like |
| --- | --- |
| `jira` | `https://<org>.atlassian.net/browse/<KEY>` |
| `github` | `https://github.com/<owner>/<repo>/issues/<n>` |
| `gitlab` | `https://gitlab.com/<group>/<project>/-/issues/<n>` |
| `custom` | Any `https://` URL |

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/results/refund-partial.json/defects \
  -H 'Content-Type: application/json' \
  -d '{"defectId":"PAY-412","defectUrl":"https://acme.atlassian.net/browse/PAY-412","trackerType":"jira"}'
```

```json
{"id":"…","message":"Defect linked to test result"}
```

The stored `DefectLink` also records a `linkId` and a `linkedAt`; both are derived by the API and
are not accepted in the request body. The `linkId` is an opaque string — it is never validated as a
document name. Linking the same defect twice answers `409 conflict`. Deleting a link answers `200`;
deleting it again answers `404 not_found`.

## Worked example

Starting from a running instance on `http://localhost:3100` (see
[Installation and first project](getting-started.md)), with the `Payments` project and
`refund-partial.json` case from
[Projects, suites, and cases](projects-suites-and-cases.md) present. Authentication is assumed off;
with it on, add `-H "Authorization: Bearer $TOKEN"`.

**1. Create a run in the project, include the suite, and include the project-owned case.**

```sh
curl -s -X POST http://localhost:3100/projects/Payments.json/test_runs \
  -H 'Content-Type: application/json' \
  -d '{"testRunId":"run-2026-09-14.json","name":"Release 4.2 regression","projects":["Payments.json"],"tags":["regression"]}'

curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/test_suites \
  -H 'Content-Type: application/json' -d '{"suiteId":"Refunds.json"}'

curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/test_cases \
  -H 'Content-Type: application/json' -d '{"testCaseId":"smoke-checkout.json"}'
```

The suite carries `refund-partial.json`; `smoke-checkout.json` lives directly in the project, so it
needs its own include before a result can be recorded for it.

**2. Record two outcomes.**

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/results \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"refund-partial.json","status":"Failed","notes":"Credited the full amount"}'

curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/results \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"smoke-checkout.json","status":"Passed","durationMs":1500}'
```

**3. Link the defect.**

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/results/refund-partial.json/defects \
  -H 'Content-Type: application/json' \
  -d '{"defectId":"PAY-412","defectUrl":"https://acme.atlassian.net/browse/PAY-412","trackerType":"jira"}'
```

**4. Edit the source case and confirm the run does not move.**

```sh
curl -s -X PUT http://localhost:3100/test_cases/refund-partial.json \
  -H 'Content-Type: application/json' -d '{"title":"Partial refunds (rewritten)"}'

curl -s http://localhost:3100/test_runs/run-2026-09-14.json
```

The run still shows the title it executed, its recorded results and its `caseVersions` pin.

**5. Read the run's contribution to a report.**

```sh
curl -s 'http://localhost:3100/reports/summary?projectId=Payments.json'
```

See [Result imports and reports](imports-and-reports.md) for what those numbers mean.

## Things that catch people out

| Symptom | Cause |
| --- | --- |
| The run shows an old case title | That is the point — the run is a snapshot, not a live view |
| Two outcomes for one case collapse into one | A run holds one outcome per case; the second call **merges** into the first |
| `400 invalid_status` | The status was not one of `Passed`, `Failed`, `Blocked`, `Untested`, `Retest` |
| `404 not_found` on `POST /test_runs/{id}/results` | The run does not hold that case. Include it first with `POST /test_runs/{id}/test_cases`, or import the report that ran it |
| `400 invalid_request` on a result body | An unknown field — the request schema is closed, so `attachments`, `defectLinks` and a GUI's `comment`/`duration` names are refused |
| `400 invalid_request` on `durationMs` | `durationMs` must be a non-negative integer, not a negative or fractional number |
| `409 conflict` linking a defect | That defect is already linked to this result |
| `404 not_found` deleting a defect link | The link was already removed, or never existed |
| `400` on `defectUrl` | The URL does not match the shape required by the chosen `trackerType` |
| A run is missing from `GET /test_runs` | The listing can be filtered by `?tags=` or `?configuration=`; check what you sent |
| `400 invalid_request` on `POST /test_runs` | The flat creation route is retired: create the run inside its project with `POST /projects/{id}/test_runs` |
| `404 not_found` on `POST /projects/{id}/test_runs` | No project has that `id` — a run is created inside a project that exists |
| `409 conflict` on `GET /test_runs/{id}` | Two or more projects hold a run with that id; name the home with `GET`/`PUT`/`DELETE /projects/{id}/test_runs/{run_id}` |
| `403 forbidden` on a run | Writing a run requires `editor` in its **home project** and in **every** project its `projects` array names; reading requires `viewer` in the same set. A run that names no project is governed by its home alone — it is not thereby open to every caller |

## Next

| I want to… | Read |
| --- | --- |
| Import results from JUnit XML or JSON instead of typing them | [Result imports and reports](imports-and-reports.md) |
| Link a run's results to a milestone | [Milestones](milestones.md) |
| Understand what the run pinned version-wise | [Case versioning and history](case-versioning-and-history.md) |
| Read the coverage and summary reports | [Result imports and reports](imports-and-reports.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for every run, result and defect-link route,
parameter and schema named here; the storage concept in the
[repository README](../../README.md#storage-concept) for the `projects/<project>/test_runs/<id>.json`
layout and the point-in-time rule; [`docs/architecture/adr-storage-layout-v3.md`](../architecture/adr-storage-layout-v3.md)
for why runs live inside their project and are governed by it; the
[compatibility contract](../contracts/api-compatibility.md) for the
defect-link plans (#87, #88), the run case-version capture plan (#92) and the run-result merge and
membership plan (#284, #285). Where this page and one of those disagree, the source wins and this
page is a bug.*
