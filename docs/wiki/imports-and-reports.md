# Result imports and reports

Recording outcomes one `curl` at a time does not scale past a smoke test. Two import routes take a
whole result set in one call — JUnit XML from your existing CI, or a JSON array from a script — and
two report routes turn the recorded runs into numbers.

Exact schemas and status codes live in the Swagger UI at `/api-docs` and
[`openapi.json`](../../openapi.json).

## Importing JUnit XML

`POST /test_runs/{id}/import/junit` (`importJUnitResults`) takes the XML **as the raw body**:

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/import/junit \
  -H 'Content-Type: application/xml' \
  --data-binary @results.xml
```

```json
{"imported":3,"skipped":1,"errors":0,"duplicates":1,"summary":{"passed":2,"failed":1,"blocked":0}}
```

Rules the importer applies:

- **Any depth is searched.** A `<testcase>` inside nested suites is found; the wrapper structure
  does not matter.
- **The mapping is fixed.** A `<failure>` or `<error>` element makes the case `Failed`; a
  `<skipped>` element makes it `Blocked`; anything else is `Passed`.
- **A `<testcase>` is matched to a run case by its identifier.** A case under test that the XML does
  not mention stays untouched.
- **The import never overwrites.** A case that already has a recorded result in this run is counted
  as a duplicate and skipped.

## Importing JSON results

`POST /test_runs/{id}/import/json` (`importJsonResults`) takes `application/json` in one of two
shapes: a bare array, or an object with a `results` array.

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/import/json \
  -H 'Content-Type: application/json' \
  -d '{"results":[
        {"testCaseId":"refund-partial.json","status":"Failed","notes":"Credited the full amount"},
        {"testCaseId":"smoke-checkout.json","status":"Passed","durationMs":1500}
      ]}'
```

Each entry is an `ImportEntry`:

| Field | Notes |
| --- | --- |
| `testCaseId` | **Required.** The case this entry is about |
| `status` | **Required.** Only `Passed`, `Failed` or `Blocked` |
| `notes` | Free prose |
| `timestamp` | Stamped with the current Unix seconds when omitted |

- **Only three statuses are accepted.** `Untested` and `Retest` have no meaning in an import and are
  rejected as `400 invalid_status`.
- **The import is strict and atomic.** An entry the importer cannot use fails the whole request
  rather than being counted, so `errors` in the summary is always `0`.
- **It never overwrites** — a case that already has a result in this run is counted as a duplicate.

## Reading the import summary

Both routes answer `200` with an `ImportSummary`:

| Field | Meaning |
| --- | --- |
| `imported` | Entries newly recorded |
| `duplicates` | Entries skipped because the case already had a result in this run |
| `errors` | Entries the importer refused; always `0` for a successful import |
| `skipped` | `duplicates + errors` |
| `summary` | `ImportCounts` — `passed`, `failed`, `blocked` for the entries that were imported |

So `imported + skipped` is the number of cases the payload mentioned, and
`summary.passed + summary.failed + summary.blocked` equals `imported`.

## The coverage report

`GET /reports/coverage` (`getCoverageReport`) answers how much of the tree is under test:

```sh
curl -s 'http://localhost:3100/reports/coverage?projectId=Payments.json'
```

```json
{
  "projectId": "Payments.json",
  "totalCases": 3,
  "suites": [
    {"suiteId":"Refunds.json","name":"Refunds","caseCount":2},
    {"suiteId":"Regression.json","name":"Regression","caseCount":1}
  ]
}
```

| Field | Notes |
| --- | --- |
| `projectId` | Echoed back only when the request was scoped to a project |
| `totalCases` | Every case counted for the scope |
| `suites` | One entry per suite, with the number of cases it holds |

`?projectId=` is the **only** parameter. A case owned directly by a project counts in `totalCases`
but belongs to no suite, so **`totalCases` can be larger than the sum of the suite counts** — that
gap is exactly the cases living outside a suite.

## The summary report

`GET /reports/summary` (`getSummaryReport`) aggregates recorded results across runs:

```sh
curl -s 'http://localhost:3100/reports/summary?projectId=Payments.json'
```

```json
{"total":3,"passed":1,"failed":1,"blocked":0,"untested":1,"passPercentage":33.33,"totalDurationMs":5700}
```

| Field | Notes |
| --- | --- |
| `total` | Every result counted — including ones whose status is `Retest` |
| `passed`, `failed`, `blocked`, `untested` | Per-status counts |
| `passPercentage` | `passed / total`, unrounded |
| `totalDurationMs` | Sum of the results' `durationMs`; a result without one contributes zero |

Five parameters scope it, and they compose:

| Parameter | Meaning |
| --- | --- |
| `projectId` | Restrict to one project |
| `milestoneId` | Restrict to a milestone's referenced runs |
| `configurationId` | Restrict to runs linking that configuration |
| `from`, `to` | Inclusive lower and upper bounds on a run's own `timestamp`, as `YYYY-MM-DD` or an ISO-8601 value |

## Worked example

Starting from a running instance on `http://localhost:3100` (see
[Installation and first project](getting-started.md)), with the run from
[Test runs and results](test-runs-and-results.md) present and two of its cases still unrecorded.
Authentication is assumed off; with it on, add `-H "Authorization: Bearer $TOKEN"`.

**1. Write a small JUnit file.**

```sh
cat > results.xml <<'XML'
<testsuites>
  <testsuite name="checkout">
    <testcase name="refund-partial.json"/>
    <testcase name="smoke-checkout.json"><failure message="no order"/></testcase>
  </testsuite>
</testsuites>
XML
```

**2. Import it.**

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/import/junit \
  -H 'Content-Type: application/xml' --data-binary @results.xml
```

```json
{"imported":2,"skipped":0,"errors":0,"duplicates":0,"summary":{"passed":1,"failed":1,"blocked":0}}
```

**3. Import the same file again** — nothing is overwritten:

```json
{"imported":0,"skipped":2,"errors":0,"duplicates":2,"summary":{"passed":0,"failed":0,"blocked":0}}
```

**4. Read the reports.**

```sh
curl -s 'http://localhost:3100/reports/coverage?projectId=Payments.json'
curl -s 'http://localhost:3100/reports/summary?projectId=Payments.json'
```

## Things that catch people out

| Symptom | Cause |
| --- | --- |
| `400 invalid_status` from the JSON import | The entry used `Untested` or `Retest`, which an import does not accept |
| `errors` is always `0` | The JSON import is strict and atomic: a bad entry fails the request instead of being counted |
| `imported` is `0` and `duplicates` matches the payload | Every case already had a result in this run — imports never overwrite |
| A `<testcase>` from the XML is missing | Its identifier did not resolve to a case already included in the run |
| The JUnit import answers `400` | The body was not framed as `application/xml`, or the XML did not parse |
| `totalCases` exceeds the sum of `suites[].caseCount` | Cases owned directly by a project count in `totalCases` and belong to no suite |
| `passPercentage` has no rounding | It is reported unrounded on purpose; format it in the client |
| `total` looks one higher than the statuses you counted | `total` includes results whose status is `Retest`, which has no column of its own |

## Next

| I want to… | Read |
| --- | --- |
| Understand how a run records results in the first place | [Test runs and results](test-runs-and-results.md) |
| Scope the summary report to a milestone | [Milestones](milestones.md) |
| See which configuration a run was executed under | [Tags and configurations](tags-and-configurations.md) |
| Understand the version a run pinned | [Case versioning and history](case-versioning-and-history.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for the import and report routes, their
parameters and the `ImportSummary`, `CoverageReport` and `SummaryReport` schemas named here; the
[compatibility contract](../contracts/api-compatibility.md) for the JUnit import plan (#85), the
JSON result import plan (#86), the coverage report plan (#94) and the summary report plan (#95).
Where this page and one of those disagree, the source wins and this page is a bug.*
