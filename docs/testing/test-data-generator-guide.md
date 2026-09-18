# Test-data generator guide

`scripts/seed.mjs` is the test-data generator: it builds a complete, representative Tucano Test
environment against a running deployment, and `scripts/teardown.mjs` is its inverse. This document is
how to **use** it for each of its three audiences and how to **extend** it when the API gains a
feature.

The dataset itself — every identifier, every document, every call that produces it — is specified by
[docs/testing/seed-dataset-spec.md](seed-dataset-spec.md). That specification is the contract; this
guide is the operator's and contributor's view of it. Where the two disagree, the specification wins
and this guide is a bug.

- **Generator:** [`scripts/seed.mjs`](../../scripts/seed.mjs) — issue
  [#193](https://github.com/TucanoTechnology/TucanoTestAPI/issues/193)
- **Teardown:** [`scripts/teardown.mjs`](../../scripts/teardown.mjs) — issue
  [#194](https://github.com/TucanoTechnology/TucanoTestAPI/issues/194)
- **Specification:** [seed-dataset-spec.md](seed-dataset-spec.md) — issue
  [#192](https://github.com/TucanoTechnology/TucanoTestAPI/issues/192)
- **Parent epic:** [#169](https://github.com/TucanoTechnology/TucanoTestAPI/issues/169)

## 1. What the generator produces

One run creates, over the HTTP API alone:

- two projects: `checkout.json`, with tags and two directly owned cases, and `payments.json`, with one;
- four suites plus a duplicate of the checkout suite: `checkout.json` gets `smoke.checkout.json`, a
  second suite `regression.checkout.json` that the run deliberately leaves empty, and
  `portable.checkout.json`; `payments.json` gets `smoke.payments.json`. `portable.checkout.json` is
  then moved into `payments.json` and copied back, so it ends up in both projects;
- eight test cases, every one carrying ordered steps and at least one attachment — fifteen uploads in
  all, eight on the cases and seven on their steps, with two cases carrying two steps each;
- a run with a linked configuration, pinned case membership and a recorded result for every status —
  `Passed`, `Failed` (with notes and a duration), `Blocked`, `Retest` — with `Untested` left implicit,
  and a result recorded twice to show replacement;
- four defect links, one per tracker type, with the GitHub one unlinked again;
- a second run whose results arrive by JSON and JUnit import;
- a milestone deriving its progress from the first run;
- every containment and placement shape the API offers: a copy per parent pair (`TC-LOGIN-1`
  suite→project, `TC-ORDERS-1` project→project, `TC-CATALOG-1` project→suite, `TC-SEARCH-1`
  suite→suite), four moves chained onto `TC-MOVE-1` so one case passes through all four move
  directions, and a no-op move of `TC-PROJECT-1`.

The result is the tree of specification §2 below `TUCANO_DATA_DIR`. The two properties that matter
for every audience below are that it is **generated, never stored** (so it cannot drift from the API)
and that it is **scoped in both directions** (the seed refuses to overwrite its own identifiers;
teardown removes only them).

## 2. The three audiences

### 2.1 A new user: a demo environment

Use it to get a populated deployment in one command rather than clicking through an empty GUI.

```sh
TUCANO_AUTH_REQUIRED=true \
TUCANO_JWT_SECRET='<at least 32 bytes>' \
TUCANO_BOOTSTRAP_USERNAME=admin \
TUCANO_BOOTSTRAP_PASSWORD=admin-password \
  docker compose up -d --build

node scripts/seed.mjs http://localhost:3100
```

Then browse the GUI at `http://localhost:8080`, or Swagger at `http://localhost:3100/api-docs`, and
find every feature with real content behind it. The [repository README's storage
concept](../../README.md#storage-concept) explains the tree the run produced; if you mounted a host
folder, you can read the same JSON files the API wrote.

Point it at a **throwaway** deployment when you are experimenting: the seed is not idempotent by
design, so a second run onto the same volume is refused until teardown has cleared the first.

### 2.2 A developer: a fixture that cannot drift

Use it as the fixture for work on the API, the storage layout, or a client. A checked-in fixture
silently goes stale; this one is re-derived by calling the same routes a client calls, so a change to
a document shape or a folder layout shows up in the generated tree rather than in a file nobody
refreshed.

The round trip is:

```sh
node scripts/seed.mjs  http://localhost:3000   # build it
# … change the API, rebuild, restart …
node scripts/teardown.mjs http://localhost:3000   # remove exactly what the seed created
node scripts/seed.mjs  http://localhost:3000   # build it again over the new build
```

Teardown scopes every removal to the identifiers the seed fixed: it reads each entity back before
deleting it, names anything it finds that the seed did not create, and exits non-zero when it could
not resolve something, so a shared volume is never mistaken for a clean one. Compare the regenerated
tree against specification §2 to see what your change did to the layout.

### 2.3 QA and reviewers: a test bed

Use it as the scratch environment for exercising a candidate build, and as the acceptance checklist
for the dataset itself.

- **The coverage matrix** (specification §1) is the list of features that must each have a seeded
  example. It is the answer to "is this dataset still representative?" after a feature lands.
- **The validation step** (specification §3 step 12) is the set of assertions a seeded deployment must
  satisfy: health; every document readable back through a `GET` route that resolves it; every case
  carrying ordered steps, at least one case-level attachment, the expected step attachments, a
  `version` of 2 or more and a reported revision; the composed cases and the two-homed suite refused
  with `409` on their bare lookup routes, yet still readable through the listing of a parent that
  holds them; each project's case listing holding exactly the seed's cases; milestone progress with
  five buckets whose `totalCases` matches the cases the run declares; both report scopes; the
  `?tags=` and `?configuration=` filters; `GET /auth/me`; and a `403 forbidden` for an
  under-privileged write.
- **Reproducing a report or a bug** is a seed run away, and teardown puts the deployment back exactly
  as it was found.

Seed and validate a candidate in the same scratch deployment before promoting it; the surrounding
promotion procedure is
[docs/deployment/canary-validation-and-rollback.md](../deployment/canary-validation-and-rollback.md).

## 3. Running it

Both scripts need Node.js 18+ (native `fetch`, ES modules) and a deployment started **with auth
enforced**, because each signs in as the bootstrap account:

| Variable | Used by | Purpose |
| --- | --- | --- |
| `TUCANO_BOOTSTRAP_USERNAME` / `TUCANO_BOOTSTRAP_PASSWORD` | both | The account each script signs in as. Required. |
| `TUCANO_API_URL` | both | Base URL when no argument is given. The seed probes `http://localhost:3100`, `http://localhost:8080/api`, then `http://localhost:3000`; teardown probes the same list but **never guesses** — it fails when no candidate answers `/health`. |
| `TUCANO_SEED_VIEWER_PASSWORD` | seed | Password for the seeded non-administrator `viewer` account. |
| `TUCANO_SEED_AUTH_CMD` | seed | Command line that seeds the `viewer` account and its grants (see [§4](#4-the-auth-exception)). When unset, that step is skipped with a notice. |
| `TUCANO_SEED_VIEWER_USERNAME` | teardown | The account to remove. Defaults to `viewer`. |
| `TUCANO_UNSEED_AUTH_CMD` | teardown | Command line that removes the account and its grants. When unset, that step is reported as not run and the run exits non-zero, because the account would otherwise survive. |

`TUCANO_AUTH_REQUIRED`, `TUCANO_JWT_SECRET` (or `TUCANO_JWT_SECRET_FILE`) and the two bootstrap
settings are the deployment's, not the scripts': with auth off there is no token to obtain and
`POST /auth/login` answers `storage_error`, so a seed run against a misconfigured deployment fails
loudly at step 0. The full settings contract is in the [repository
README](../../README.md#authentication).

### 3.1 Repeated runs

The seed is **not idempotent, by design**. The specification fixes the identifiers it creates, and
placing a case onto an identifier the target parent already holds is a `409`, so a second run onto the
same volume cannot complete. Rather than fail half-way through, the script checks its own identifiers
up front and refuses, pointing at teardown:

```sh
node scripts/teardown.mjs   # the scoped "clear first"
node scripts/seed.mjs       # then seed again
```

### 3.2 Removing it again

`scripts/teardown.mjs` removes exactly what the seed created — specification §4's ordered cleanup —
and nothing else.

```sh
TUCANO_BOOTSTRAP_USERNAME=admin \
TUCANO_BOOTSTRAP_PASSWORD=admin-password \
  node scripts/teardown.mjs http://localhost:3000
```

It exits `0` when everything the seed created is gone or was never there, and `1` when something could
not be resolved, naming each item it left in place and why. Re-running it over an already-clean volume
is a success: a missing entity is a settled teardown, not a failure.

`scripts/clear-data.mjs` is the opposite tool and is deliberately *not* what this does: it empties the
whole of every collection (milestones, runs, suites, cases, projects), removes no configuration by a
step of its own — deleting a project cascades to the configurations it holds, so the ones inside a
listed project go with it rather than being addressed by a bare id whose project would be ambiguous —
leaves accounts and grants alone, and falls back to a guessed base URL. Use it only when you want the
whole volume emptied and do not care what else was in it. The three scripts are compared in the
[storage concept and API reference](../reference/storage-and-api.md#test-data-cleanup).

## 4. The auth exception

The API publishes no route that creates an account or records a project grant, so rows 25–27 of the
coverage matrix cannot be satisfied over HTTP like everything else. The server binary therefore
exposes two subcommands that write through the same `AuthStore` the running server reads:

```sh
# the seed's half: create the account and its grants (idempotent)
TUCANO_DATA_DIR=/data ./tucano-test seed-auth \
    --username viewer --password viewer-password \
    --grant checkout.json=owner

# the teardown's half: forget the account and the grants it holds on the named projects
TUCANO_DATA_DIR=/data ./tucano-test unseed-auth \
    --username viewer --grant checkout.json
```

The scripts invoke them through `TUCANO_SEED_AUTH_CMD` and `TUCANO_UNSEED_AUTH_CMD`, so each takes the
form of a command *line* rather than a binary path. Against the Compose volume:

```sh
TUCANO_SEED_AUTH_CMD='docker compose exec -T api tucano-test seed-auth' \
TUCANO_SEED_VIEWER_PASSWORD=viewer-password \
  node scripts/seed.mjs http://localhost:3000

TUCANO_UNSEED_AUTH_CMD='docker compose exec -T api tucano-test unseed-auth' \
  node scripts/teardown.mjs http://localhost:3000
```

Three properties keep this exception honest:

- **`seed-auth` is idempotent** where the dataset is not: an existing account keeps its password and
  only the grants it is missing are added, because a re-run must not silently reset a password.
- **`unseed-auth` is scoped like teardown**: it requires at least one `--grant`, removes only the
  grants it is told about rather than every grant the account happens to hold, refuses the bootstrap
  account outright, and reports anything it cannot find as left in place instead of guessing.
- **`GET /auth/me` closes the loop.** After seeding, the script signs in as the seeded account and
  asserts the API reports no system-administrator flag and `owner` on `checkout.json` alone — the
  project the account is granted, with no role recorded against `payments.json`. That single grant is
  deliberate: a viewer who reaches one project and not the other is what makes the isolation the
  validation step checks observable, and it proves the files were written in the format the server
  actually honours.

The gap itself — that auth accounts and grants have no HTTP route, and what closing it would require —
is recorded in specification §5 and tracked by the epic rather than papered over here.

## 5. Extending the generator

A new API feature reaches the dataset through five coordinated edits. Work them in this order: the
specification is the contract, the generator implements it, the README describes it, and writing code
before the row is how the two drift apart.

1. **Add the fixture to `scripts/fixtures/`**, if the feature needs a file. The seed's attachments and
   its JUnit report live there; a feature that needs no file skips this step.
2. **Add a matrix row** to specification §1 with the row shape `#`, feature, seeded example, producing
   call, on-disk evidence. A row with no example is a gap, not a deferral. If the feature stores a new
   file or folder, name it in §2's target tree as well.
3. **Add the calls to specification §3**, in the step whose resources they depend on, in the order the
   dependencies require. The step order in §3 is a dependency order, not a preference: a call that
   addresses an entity by an identifier read back from an earlier response has to run before
   step 11 gives that entity a second home, because a bare identifier that resolves to two parents
   answers `409` from then on.
4. **Implement the calls** in `scripts/seed.mjs` as a `stepN…` function and add it to the sequence in
   `runSeed()`. Rules that keep the generated tree a shape the API would write:
   - call the API over HTTP, as every other step does — the only permitted exception is the auth
     accounts and grants of [§4](#4-the-auth-exception);
   - read anything the API derived (a duplicate's id, a defect link's id) back from the response, and
     assert it is present, rather than inventing a value;
   - let `call()` throw on an unexpected status, so a failure names the call and its response instead
     of leaving a half-written tree behind;
   - keep placement last. Every write that names an entity by its own identifier — a `PUT`, an
     attachment upload, a run's membership — belongs before step 11, because placing an entity gives
     it a second home and makes the bare identifier ambiguous.
5. **Extend `scripts/teardown.mjs`** so the run stays reversible: add the new identifiers to the
   constants at the top of the file and a removal to the step whose dependencies allow it. Every
   removal goes through the guard mechanism, and anything the teardown cannot resolve is reported as
   kept rather than guessed at.

Then **assert it in the validation step** — specification §3 step 12 — so the new row fails loudly
instead of going stale. That is the check the specification's stale-matrix paragraph promises, and it
is what turns "keep the matrix up to date" from an intention into a check.

### 5.1 A worked example

Rows 14 and 15 of the matrix are the recorded results: every status, and the replacement of one
result by a second call for the same case. They took:

- no fixture — nothing is uploaded;
- two matrix rows, and no new entry in §2's tree, because a result lives inside the run's own
  `test_runs/<id>.json` rather than in a file of its own;
- the `POST /test_runs/{id}/results` calls already sequenced into `step7Results` in §3, including the
  `Blocked`-then-`Failed` pair that demonstrates replacement;
- no teardown change at all: a run's results are removed with the run, so the existing `step2Runs`
  removal already covers them.

That is the cheap case. A feature that stores a new kind of file is the expensive one, because it
touches all five edits, and `step5Attachments` (a new file on an existing entity) is the example to
copy there.

### 5.2 The validation step

The assertions of specification §3 step 12 live in
[`scripts/validate-seed.mjs`](../../scripts/validate-seed.mjs) (issue
[#195](https://github.com/TucanoTechnology/TucanoTestAPI/issues/195)), which runs against a seeded
deployment rather than inside the generator, so the seed never validates its own work. A feature the
seeded dataset must expose also belongs in that script: without an assertion the new row passes over
a deployment that no longer has the feature.

[`scripts/demo.sh`](../../scripts/demo.sh) is the one-command path that ties the three together —
bring a stack up, seed it, run `scripts/smoke.sh`, then validate — and
[`scripts/check-matrix.mjs`](../../scripts/check-matrix.mjs) is the static half CI runs, which
catches a route that reaches no matrix row without starting anything.
