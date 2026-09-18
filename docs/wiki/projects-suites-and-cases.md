# Projects, suites, and cases

Tucano Test stores three things as containers: a **project** holds all the other entities, a
**suite** groups related cases inside a project, and a **case** is the individual test. This page
explains that hierarchy, how creation works when the parent lives in the route, and how membership
is stored — because the last one surprises people who expect an array in the document.

Nothing on this page replaces the contract. For a route's exact parameters, body schema and status
codes, open the Swagger UI at `/api-docs` or read [`openapi.json`](../../openapi.json).

## The shape of the tree

```text
TUCANO_DATA_DIR/
└── projects/
    └── <project>/
        ├── project.json                 project details
        ├── <test case>/                 a case owned directly by the project
        │   └── test-case.json
        ├── <test suite>/
        │   ├── suite.json              suite details
        │   └── <test case>/
        │       └── test-case.json
        ├── test_runs/<id>.json          the project's runs
        ├── milestones/<id>.json         the project's milestones
        └── configurations/<id>.json     the project's configurations
```

Each entity is a **folder** holding a JSON document plus any supplementary files of its own.
Test runs, milestones, and configuration documents are not folders — they are flat `<id>.json`
files inside the project that owns them, under `projects/<project>/test_runs/`,
`projects/<project>/milestones/` and `projects/<project>/configurations/`. The `test_runs`,
`milestones` and `configurations` names are reserved inside a project: a suite or a case created
directly in a project cannot take one, and an attempt is refused as `409 conflict`.

| Concept | What it contains | Where it lives |
| --- | --- | --- |
| **Project** | The container for the work under test. Holds suites and may hold cases directly. | `projects/<project>/` |
| **Test suite** | A reusable grouping of cases, with its own suite-level data. Always inside a project. | `projects/<project>/<suite>/` |
| **Test case** | One test: details, steps, expected results, supplementary files. Inside a project or a suite. | `<case>/` |
| **Test run**, **milestone**, **configuration** | Flat documents, not containers. | `projects/<project>/{test_runs,milestones,configurations}/<id>.json` |

A case is not required to belong to a suite. A project can own cases directly — handy for
one-off checks that do not belong in a reusable group.

## Membership is the folders

Reading a project's marker on disk shows `testSuites: []` and a suite marker shows `testCases: []`
even when children exist. Those arrays are **legacy-shaped placeholders kept for compatibility**;
the real membership is the child folders themselves.

The API resolves the difference for you on reads:

| Route | What it returns |
| --- | --- |
| `GET /projects/{id}` (`getProject`) | The project with `testSuites` assembled from the folders it holds (each suite recursively assembled), plus a response-only `testCases` field for cases owned directly — omitted entirely when the project owns none |
| `GET /test_suites/{id}` (`getTestSuite`) | The suite with `testCases` assembled from its child folders |
| `GET /test_cases/{id}` (`getTestCase`) | The case document as stored |

Because membership lives in the folders, a stored marker can never contradict the tree. It also
means a `testSuites` array sent in a create or update body is accepted for compatibility and then
discarded — the request schemas say so explicitly — so never try to add a suite by editing the
project document.

## Creating things: the parent is in the route

There is no top-level `POST /test_suites`, `POST /test_cases`, `POST /test_runs`,
`POST /milestones` or `POST /configurations`. A suite, case, run, milestone or configuration is
created by posting to its parent project's collection route:

| Route | Creates |
| --- | --- |
| `POST /projects` (`createProject`) | A project |
| `POST /projects/{id}/test_suites` (`addProjectTestSuite`) | A suite in a project, or places an existing suite there |
| `POST /projects/{id}/test_cases` (`addProjectTestCase`) | A case directly in a project, or places an existing case there |
| `POST /test_suites/{id}/test_cases` (`addTestSuiteCase`) | A case in a suite, or places an existing case there |
| `POST /projects/{id}/test_runs` (`addProjectTestRun`) | A run in a project |
| `POST /projects/{id}/milestones` (`addProjectMilestone`) | A milestone in a project |
| `POST /projects/{id}/configurations` (`addProjectConfiguration`) | A configuration in a project |

The retired flat routes answer `400 invalid_request` naming their replacement, so an old client
fails loudly instead of writing to a tree the API no longer maintains.

Identifiers are **derived from the name** when you omit them: a suite posted with `{"name":…}`
gets the id `<name>.json`, and a project posted with `{"name":…}` gets the id `<name>.json`. A
supplied identifier must be a single path segment ending in `.json`; anything with a separator, a
`..`, or an absolute path is rejected as `invalid_id`. A case is the exception — its `testCaseId`
is not derived from anything, because a case has a `title` (the displayed name) as well as an
identifier (the handle).

## Worked example

Starting from a running instance on `http://localhost:3100` (see
[Installation and first project](getting-started.md)). Authentication is assumed off; with it on,
add `-H "Authorization: Bearer $TOKEN"` to each call.

**1. Create a project.**

```sh
curl -s -X POST http://localhost:3100/projects \
  -H 'Content-Type: application/json' \
  -d '{"name":"Payments","description":"The checkout and refund flows"}'
```

```json
{"id":"Payments.json","message":"Resource created"}
```

**2. Create a suite inside it.**

```sh
curl -s -X POST http://localhost:3100/projects/Payments.json/test_suites \
  -H 'Content-Type: application/json' \
  -d '{"name":"Refunds"}'
```

```json
{"id":"Refunds.json","message":"Test suite created"}
```

**3. Create a case inside the suite.**

```sh
curl -s -X POST http://localhost:3100/test_suites/Refunds.json/test_cases \
  -H 'Content-Type: application/json' \
  -d '{
        "testCaseId": "refund-partial.json",
        "title": "Partial refund returns the difference",
        "expectedResult": "The card is credited the refunded amount"
      }'
```

```json
{"id":"refund-partial.json","message":"Test case created"}
```

**4. Create a second case directly in the project** — no suite involved.

```sh
curl -s -X POST http://localhost:3100/projects/Payments.json/test_cases \
  -H 'Content-Type: application/json' \
  -d '{
        "testCaseId": "smoke-checkout.json",
        "title": "Checkout smoke check",
        "expectedResult": "An order is created"
      }'
```

```json
{"id":"smoke-checkout.json","message":"Test case created"}
```

**5. Read the project back.**

```sh
curl -s http://localhost:3100/projects/Payments.json
```

```json
{
  "name": "Payments",
  "projectId": "Payments.json",
  "description": "The checkout and refund flows",
  "testSuites": [
    {
      "name": "Refunds",
      "suiteId": "Refunds.json",
      "testCases": []
    }
  ],
  "testCases": [
    {
      "expectedResult": "An order is created",
      "lastModified": "…",
      "testCaseId": "smoke-checkout.json",
      "title": "Checkout smoke check",
      "version": 1
    }
  ]
}
```

Three things to notice, because they catch almost every client:

- The suite entry's own `testCases` is empty — the suite's cases are assembled when you read the
  **suite**, not when you read the project.
- The directly owned case appears in the response-only `testCases` field.
- The case carries `version` and `lastModified`, which the API stamps. See
  [Case versioning and history](case-versioning-and-history.md).

Reading the suite completes the picture:

```sh
curl -s http://localhost:3100/test_suites/Refunds.json
```

```json
{
  "name": "Refunds",
  "suiteId": "Refunds.json",
  "testCases": [
    {
      "expectedResult": "The card is credited the refunded amount",
      "lastModified": "…",
      "testCaseId": "refund-partial.json",
      "title": "Partial refund returns the difference",
      "version": 1
    }
  ]
}
```

**6. Move the case between parents, or copy it.** Inclusion semantics are their own page:
[Composing and duplicating](composing-and-duplicating.md).

**7. Remove children and the project.**

```sh
curl -s -X DELETE http://localhost:3100/projects/Payments.json/test_cases/smoke-checkout.json
curl -s -X DELETE http://localhost:3100/test_suites/Refunds.json/test_cases/refund-partial.json
curl -s -X DELETE http://localhost:3100/test_suites/Refunds.json
curl -s -X DELETE http://localhost:3100/projects/Payments.json
```

The parent-scoped delete routes (`removeProjectTestCase`, `removeProjectTestSuite`,
`removeTestSuiteCase`) remove one child from one parent, which is the safe way to delete a child
that was copied into several parents. Deleting a suite removes its cases with it; deleting a
project removes everything under it.

## Identifiers are unique where they live

| Entity | Uniqueness |
| --- | --- |
| Project | Globally unique |
| Suite | Unique inside its project |
| Case | Unique inside its parent (project or suite) |
| Test run, milestone, configuration | Unique inside the project that holds it |

Copy-on-include can put the **same** case id under several parents, so a document-level route such
as `GET /test_cases/{id}` (`getTestCase`) operates on the single occurrence when the id resolves to
one and answers `409 conflict` when it is ambiguous, naming the parent-scoped routes that are
unambiguous. Listing routes never fail on duplicates; they de-duplicate. The attachment routes
behave the same way on their bare form, and each of them also has a parent-scoped mirror under
`/projects/{id}/test_cases/{case_id}/…` and `/test_suites/{id}/test_cases/{case_id}/…` — see
[Steps and attachments](steps-and-attachments.md).

Reads are global: `GET /test_cases/{id}` and `GET /test_suites/{id}` search the whole tree, so an
entity is findable without knowing its home. There is no global listing route for cases — list them
through a project or a suite.

## Filtering and listing

| Route | Supports |
| --- | --- |
| `GET /projects` (`listProjects`) | `?filter=`, `?tags=` |
| `GET /projects/{id}/test_suites` (`listProjectTestSuites`) | — |
| `GET /projects/{id}/test_cases` (`listProjectTestCases`) | — |
| `GET /projects/{id}/test_runs` (`listProjectTestRuns`) | — |
| `GET /projects/{id}/milestones` (`listProjectMilestones`) | — |
| `GET /projects/{id}/configurations` (`listProjectConfigurations`) | — |
| `GET /test_suites/{id}/test_cases` (`listTestSuiteCases`) | — |

`?filter=` is a case-insensitive substring match on resource **identifiers**. The parent-scoped
listings take no filter; see [Tags and configurations](tags-and-configurations.md) for `?tags=`.

## Container updates and partial writes

`PUT /projects/{id}` (`updateProject`), `PUT /test_suites/{id}` (`updateTestSuite`) and
`PUT /test_cases/{id}` (`updateTestCase`) are **partial** updates: the fields you send replace the
stored ones and everything else is kept. `null` counts as "not supplied". An array you send
replaces the stored array rather than being appended to. Unknown fields are rejected, not ignored.

`POST /projects/{id}/duplicate` (`duplicateProject`), `POST /test_suites/{id}/duplicate`
(`duplicateTestSuite`) and `POST /test_cases/{id}/duplicate` (`duplicateTestCase`) write a second
document under a new id in the same parent; the entities below the source are not copied with it. See
[Composing and duplicating](composing-and-duplicating.md) for what comes along and how the id of the
copy is derived.

## Next

| I want to… | Read |
| --- | --- |
| Add steps, expected results and attachments to a case | [Steps and attachments](steps-and-attachments.md) |
| Move a case into another parent, or bring a case's whole folder across | [Composing and duplicating](composing-and-duplicating.md) |
| Group and filter entities with tags | [Tags and configurations](tags-and-configurations.md) |
| Trace a case's edit history | [Case versioning and history](case-versioning-and-history.md) |
| Record that a case passed or failed | [Test runs and results](test-runs-and-results.md) |

---

*Sources of truth: the storage concept in the [repository README](../../README.md#storage-concept)
for the folder layout and the three composition semantics;
[`docs/architecture/adr-storage-layout-v3.md`](../architecture/adr-storage-layout-v3.md) for why
runs, milestones and configurations live inside their project;
[`openapi.json`](../../openapi.json) for
every route, parameter and schema named here; the
[compatibility contract](../contracts/api-compatibility.md) for the real-home storage plan and the
copy/move-include decisions. Where this page and one of those disagree, the source wins and this
page is a bug.*
