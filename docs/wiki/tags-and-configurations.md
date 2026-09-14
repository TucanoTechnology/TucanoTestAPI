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

## Configurations

A configuration is a flat document — `<configId>.json` under `configurations/` — not a folder. It
names an environment:

| Field | Notes |
| --- | --- |
| `configId` | **Required.** The identifier; derived as `<name>.json` when omitted |
| `name` | **Required.** The displayed name |
| `browser` | Free-form, for example `Firefox` |
| `os` | Free-form, for example `Windows 11` |
| `device` | Free-form, for example `Pixel 8` |
| `resolution` | Free-form, for example `1920x1080` |

| Route | Operation id | What it does |
| --- | --- | --- |
| `GET /configurations` | `listConfigurations` | Lists them; supports `?filter=` only |
| `POST /configurations` | `createConfiguration` | Creates one from a `TestConfigurationCreateRequest` |
| `GET /configurations/{id}` | `getConfiguration` | Reads one |
| `PUT /configurations/{id}` | `updateConfiguration` | Partial update |
| `DELETE /configurations/{id}` | `deleteConfiguration` | Removes one |

```sh
curl -s -X POST http://localhost:3100/configurations \
  -H 'Content-Type: application/json' \
  -d '{"name":"firefox","browser":"Firefox","os":"Windows 11","resolution":"1920x1080"}'
```

```json
{"id":"firefox.json","message":"Configuration created"}
```

Configurations are **installation-wide**, not project-scoped: the file sits directly under
`configurations/`, and any authenticated caller may read and write them. There is no ownership on a
configuration, so naming them by environment (`firefox.json`, `stage-chrome.json`) is what keeps
them legible.

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

**1. Create a project with tags and a configuration.**

```sh
curl -s -X POST http://localhost:3100/projects \
  -H 'Content-Type: application/json' \
  -d '{"name":"Payments","tags":["payments","smoke"]}'

curl -s -X POST http://localhost:3100/configurations \
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
| `404 not_found` on a `configId` | No `<configId>.json` under `configurations/`; remember the id is the file name, not the display `name` |

## Next

| I want to… | Read |
| --- | --- |
| Record a run and link its configuration | [Test runs and results](test-runs-and-results.md) |
| See how milestones relate to suites and runs | [Milestones](milestones.md) |
| Understand how `?filter=` differs from `?tags=` | [Projects, suites, and cases](projects-suites-and-cases.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for the `tags` fields, the `?tags=`,
`?filter=` and `?configuration=` query parameters, the configuration routes and schemas named here;
the storage concept in the [repository README](../../README.md#storage-concept) for the flat
`configurations/` layout; the [compatibility contract](../contracts/api-compatibility.md) for the
tags plan (#49) and the configurations activation plan (#69). Where this page and one of those
disagree, the source wins and this page is a bug.*
