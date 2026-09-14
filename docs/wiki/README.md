# Tucano Test — User Wiki

The Tucano Test API is a file-based test case management service: it stores projects, suites,
cases, runs, milestones, and configurations as folders and JSON documents on disk — no database —
and exposes every operation through the HTTP contract in [`openapi.json`](../../openapi.json).

This wiki is written for the people who **use** Tucano Test. It is not the developer reference: for
the storage design, the module layout, and the build commands, read the [repository
README](../../README.md); for the engineering records, see [docs/](../).

## Who are you?

| I want to… | Start here |
| --- | --- |
| Install Tucano Test and create my first project | [Installation and first project](getting-started.md) |
| Understand how projects, suites, and cases are organised | [Projects, suites, and cases](projects-suites-and-cases.md) |
| Run my first authenticated API call | [API and authentication quickstart](api-and-authentication.md) |
| Run it in production, back it up, or fix a problem | [Operations and troubleshooting](operations-and-troubleshooting.md) |

## Install and first run

| Page | What it covers |
| --- | --- |
| [Installation and first project](getting-started.md) | `docker compose up -d --build`, the ports, the data volume, authentication bootstrap, and a first project created end to end |

## Feature guides

| Page | What it covers |
| --- | --- |
| [Projects, suites, and cases](projects-suites-and-cases.md) | The container hierarchy, parent-scoped creation, and how membership is stored as folders |
| [Steps and attachments](steps-and-attachments.md) | Structured test steps, case attachments, and per-step attachments |
| [Composing and duplicating](composing-and-duplicating.md) | `copy` versus `move` inclusion semantics and duplication |
| [Tags and configurations](tags-and-configurations.md) | Tagging projects, suites, cases, and runs; the shared `?tags=` filter; environment configurations |
| [Test runs and results](test-runs-and-results.md) | Point-in-time runs, recording results, and defect links |
| [Result imports and reports](imports-and-reports.md) | JUnit XML and JSON import, plus the coverage and summary reports |
| [Milestones](milestones.md) | Milestone progress derived from referenced runs |
| [Case versioning and history](case-versioning-and-history.md) | The `version` stamp, `revisions/` snapshots, and the history endpoints |

## API consumers

| Page | What it covers |
| --- | --- |
| [API and authentication quickstart](api-and-authentication.md) | Zero to an authenticated `curl`, the error envelope, request limits, and why Swagger is normative |

## Operators

| Page | What it covers |
| --- | --- |
| [Operations and troubleshooting](operations-and-troubleshooting.md) | The data volume, scaling, backup and restore, health, rollback, and a troubleshooting FAQ |

## Sources of truth

This wiki summarises; it never overrides. When a page and one of these disagree, the source wins
and the page is a bug.

| Concern | Authoritative source |
| --- | --- |
| The HTTP contract — every route, schema, and status code | [`openapi.json`](../../openapi.json), rendered at `/api-docs` |
| Storage layout and domain semantics | [README: Storage concept](../../README.md#storage-concept) and the implementation in `src/` |
| Deployment model and rollback | [docs/deployment/deployment-guide.md](../deployment/deployment-guide.md) |
| Authentication and authorization | [docs/security/authentication-decision.md](../security/authentication-decision.md) |
| Compatibility with the legacy implementation | [docs/contracts/api-compatibility.md](../contracts/api-compatibility.md) |

## Keeping this wiki current

The wiki is hand-written Markdown under `docs/wiki/`. There is no generator and no publish step: a
page is edited in the same pull request as the change that makes it wrong, and every page names the
engineering records it derives from. Feature pages never reproduce schemas — they name the operation
exactly as `openapi.json` does and link to `/api-docs` for the details.

The full rule is recorded in
[docs/architecture/documentation-strategy.md](../architecture/documentation-strategy.md).

If you find a page that contradicts `openapi.json`, the behaviour of the running service, or one of
the engineering records above, that is a documentation defect — please
[open an issue](https://github.com/TucanoTechnology/TucanoTestAPI/issues).
