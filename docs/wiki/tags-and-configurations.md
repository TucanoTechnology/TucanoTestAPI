# Tags and configurations

Two ways to slice the same tree:

- **Tags** are free-form labels you put on a project, suite, case or run, so you can pull "everything
  smoke" out of a tree organised by feature.
- **Configurations** are the named environments a run happened in — a browser, an OS, a device, a
  resolution — so you can answer "which runs were on Firefox?".

Both are stored as JSON, both are queried with a query parameter, and neither changes the folder
hierarchy. Exact schemas and status codes live in the Swagger UI at `/api-docs` and
[`openapi.json`](../../openapi.json).

## Tags

A `tags` array is available on four document types:

| Document | Field |
| --- | --- |
| `Project` | `tags` |
| `TestSuite` | `tags` |
| `TestCase` | `tags` |
| `TestRun` | `tags` |

Tags are plain strings. They are set, replaced and cleared through the ordinary create and update
routes for each type — there is no separate tagging route:

```sh
curl -s -X POST http://localhost:3100/projects \
  -H 'Content-Type: application/json' \
  -d '{"name":"Payments","tags":["payments","smoke"]}'
```

`PUT /projects/{id}` (`updateProject`) replaces the whole array, so send every tag you want kept. A
`PUT` without `tags` leaves the stored array alone, because a partial update only touches what you
send.

## The shared `?tags=` filter

One query parameter serves every listing route that supports it:

| Route | Operation id | Supports |
| --- | --- | --- |
| `GET /projects` | `listProjects` | `?filter=`, `?tags=` |
| `GET /test_runs` | `listTestRuns` | `?filter=`, `?tags=`, `?configuration=` |

`?tags=` takes a comma-separated list and matches with **OR** semantics: a resource is kept when it
carries **at least one** of the listed tags. Matching is case-insensitive and each entry is trimmed,
so `?tags=Smoke, regression` behaves as you would hope.

```sh
curl -s 'http://localhost:3100/projects?tags=smoke,regression'
curl -s 'http://localhost:3100/test_runs?tags=smoke&configuration=firefox.json'
```

Two rules that shape what you get back:

- **A resource with no `tags` array never matches.** `?tags=` is not "everything unlabelled"; it is
  "everything carrying one of these labels".
- **`?tags=` composes with `?filter=`**, and on runs it also composes with `?configuration=`. A
  request must satisfy all of the parameters you send.

Where tags are **not** available is worth knowing, because a client that assumes otherwise gets an
empty or unhelpful listing:

| Route | Why |
| --- | --- |
| `GET /milestones`, `GET /configurations` | These documents carry no `tags` field, so no tag filter is offered |
| `GET /projects/{id}/test_suites`, `GET /projects/{id}/test_cases`, `GET /test_suites/{id}/test_cases` | Parent-scoped listings are exhaustive for that parent; no query parameters |
| `GET /projects/{id}/test_runs`, `GET /projects/{id}/milestones`, `GET /projects/{id}/configurations` | The same rule for the project-scoped collections: a sorted array of ids, no `?filter=`, `?tags=` or `?configuration=` |

## Configurations

A configuration is a flat document, inside the project that owns it, at
`projects/<project>/configurations/<configId>.json` — not a folder. It names an environment:

| Field | Notes |
| --- | --- |
| `configId` | **Required.** Always derived as `<name>.json`; a value supplied in a create or update body is accepted for wire compatibility and ignored, so the identity cannot name a document other than the one it is stored in (Issue #288) |
| `name` | **Required.** The displayed name |
| `browser` | Free-form, for example `Firefox` |
| `os` | Free-form, for example `Windows 11` |
| `device` | Free-form, for example `Pixel 8` |
| `resolution` | Free-form, for example `1920x1080` |

| Route | Operation id | What it does |
| --- | --- | --- |
| `GET /projects/{id}/configurations` | `listProjectConfigurations` | Lists one project's configurations, as a sorted array of ids |
| `POST /projects/{id}/configurations` | `addProjectConfiguration` | Creates the configuration inside the project |
| `DELETE /projects/{id}/configurations/{config_id}` | `removeProjectConfiguration` | Removes the configuration from the project |
| `GET /configurations` | `listConfigurations` | Lists them across every project; supports `?filter=` only |
| `GET /configurations/{id}` | `getConfiguration` | Reads one |
| `PUT /configurations/{id}` | `updateConfiguration` | Partial update |
| `DELETE /configurations/{id}` | `deleteConfiguration` | Removes one |

Creation is project-scoped: `POST /projects/{id}/configurations` creates the configuration in that
project and answers `201` with `{"message": "Test configuration created", "id": …}`; an unknown
project answers `404 not_found` and an id already taken in that project answers `409 conflict`. The
flat `POST /configurations` is retired and answers `400 invalid_request` naming the replacement. The
bare `GET /configurations` stays served for compatibility but is deliberately absent from
`openapi.json`.

```sh
curl -s -X POST http://localhost:3100/projects/Payments.json/configurations \
  -H 'Content-Type: application/json' \
  -d '{"name":"firefox","browser":"Firefox","os":"Windows 11","resolution":"1920x1080"}'
```

```json
{"id":"firefox.json","message":"Test configuration created"}
```

A configuration is a **project resource** like everything else. It lives in the project it was
created in — that folder is its home — and its id is unique there, so two projects may each hold a
`firefox.json`; the document routes under `/configurations/{id}` answer `409 conflict` when they
cannot tell which one you mean, and name the parent-scoped route to use instead. Reading a
configuration needs `viewer` in its home project and writing one needs `editor` there; there is no
installation-wide short-circuit, so a configuration is not readable by every authenticated caller.
Naming them by environment (`firefox.json`, `stage-chrome.json`) is still what keeps them legible.
See [Storage layout v3](../architecture/adr-storage-layout-v3.md) for the decision.

## Linking a configuration to a run

A run records which configurations it was executed under. Two routes manage the links:

| Route | Operation id | Body |
| --- | --- | --- |
| `POST /test_runs/{id}/configurations` | `addTestRunConfiguration` | `{"configId": "firefox.json"}` |
| `DELETE /test_runs/{id}/configurations/{config_id}` | `removeTestRunConfiguration` | — |

The run document's `configurations` array holds the identifiers:

```sh
curl -s -X POST http://localhost:3100/test_runs/run-2026-09-14.json/configurations \
  -H 'Content-Type: application/json' -d '{"configId":"firefox.json"}'
```

A run may link a configuration from **any** project, so linking needs `editor` in the run's home
project **and** in every project the run covers; reading the run needs `viewer` in that same set.

Once linked, `?configuration=` on `GET /test_runs` keeps the runs that link that identifier:

```sh
curl -s 'http://localhost:3100/test_runs?configuration=firefox.json'
```

A configuration that no run links yields an **empty** listing rather than an error, so an empty
answer is a statement about the runs, not about the identifier. Removing the configuration document
does not rewrite the runs that link it; clean up the links with `removeTestRunConfiguration` if you
want that.

## Worked example

Starting from a running instance on `http://localhost:3100` (see
[Installation and first project](getting-started.md)). Authentication is assumed off; with it on,
add `-H "Authorization: Bearer $TOKEN"`.

**1. Create a project with tags, then a configuration in it.**

```sh
curl -s -X POST http://localhost:3100/projects \
  -H 'Content-Type: application/json' \
  -d '{"name":"Payments","tags":["payments","smoke"]}'

curl -s -X POST http://localhost:3100/projects/Payments.json/configurations \
  -H 'Content-Type: application/json' \
  -d '{"name":"firefox","browser":"Firefox","os":"Windows 11"}'
```

**2. Find it by tag.** Only the resources that actually carry one of the tags come back:

```sh
curl -s 'http://localhost:3100/projects?tags=smoke'
```

```json
[{"name":"Payments","projectId":"Payments.json","tags":["payments","smoke"]}]
```

```sh
curl -s 'http://localhost:3100/projects?tags=nonexistent'
```

```json
[]
```

**3. Retag it, replacing the array.**

```sh
curl -s -X PUT http://localhost:3100/projects/Payments.json \
  -H 'Content-Type: application/json' -d '{"tags":["payments","regression"]}'
```

`smoke` is gone: `tags` was replaced, not merged.

## Things that catch people out

| Symptom | Cause |
| --- | --- |
| A tag filter returns resources you did not expect | `?tags=` is OR, not AND. To require several labels, filter the result client-side |
| A tag filter returns nothing for a resource you tagged | The resource's document has no `tags` array, or the tag differs by more than case and surrounding whitespace |
| A resource silently loses tags | A `PUT` sent a `tags` array that omitted them; the array is replaced wholesale |
| `?tags=` on a milestone or configuration listing does nothing | Neither document type carries tags, so neither listing offers the filter |
| `?configuration=` returns an empty list | No run links that configuration identifier — the filter is a statement about runs, not a validation of the identifier |
| `404 not_found` on a `configId` | No project has a `<configId>.json` in its `configurations/`; remember the id is the file name, not the display `name` |
| `409 conflict` on `GET /configurations/{id}` | Two or more projects hold a configuration with that id; name the home with `GET`/`PUT`/`DELETE /projects/{id}/configurations/{config_id}` |
| `400 invalid_request` on `POST /configurations` | The flat creation route is retired: create the configuration inside its project with `POST /projects/{id}/configurations` |
| `403 forbidden` on a configuration | Reading needs `viewer` and writing needs `editor` in the project that holds it; configurations are no longer readable by every authenticated caller |

## Next

| I want to… | Read |
| --- | --- |
| Record a run and link its configuration | [Test runs and results](test-runs-and-results.md) |
| See how milestones relate to suites and runs | [Milestones](milestones.md) |
| Understand how `?filter=` differs from `?tags=` | [Projects, suites, and cases](projects-suites-and-cases.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for the `tags` fields, the `?tags=`,
`?filter=` and `?configuration=` query parameters, the configuration routes and schemas named here;
the storage concept in the [repository README](../../README.md#storage-concept) for the
`projects/<project>/configurations/<configId>.json` layout;
[`docs/architecture/adr-storage-layout-v3.md`](../architecture/adr-storage-layout-v3.md) for why a
configuration lives inside its project and is governed by it; the
[compatibility contract](../contracts/api-compatibility.md) for the
tags plan (#49) and the configurations activation plan (#69). Where this page and one of those
disagree, the source wins and this page is a bug.*
