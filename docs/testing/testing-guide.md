# Testing guide

Two layers of tests, a contract of per-operation coverage the build enforces, and what CI adds on
top. The commands worth memorizing live in the [README](../../README.md); this page is the full
map.

Tests are split into two layers and both run in CI on every push and pull request:

- **Unit tests** live beside the code in `src/models.rs`, `src/storage/`, and `src/domain/`. They
  cover legacy JSON compatibility, payload validation, composition and duplication rules, milestone
  progress, atomic writes, path confinement, attachment storage, concurrent writers, and file
  permissions.
- **Integration tests** live in `tests/` and exercise the HTTP surface in-process through the Axum
  router against a temporary data directory. Each API area has its own suite:

| Suite | Covers |
| --- | --- |
| `tests/service.rs` | Health, readiness and storage diagnostics, OpenAPI document and its error contract, Swagger UI, malformed bodies, traversal rejection, the identifier and size-limit error answers, the on-disk tree layout, persistence across restarts, and the coverage table that fails when a documented operation has no covering test |
| `tests/projects.rs` | Project CRUD, validation, conflicts, error envelopes |
| `tests/suites.rs` | Test suite CRUD, parent-scoped creation, copy/move composition, ambiguity conflicts, missing resources |
| `tests/runs.rs` | Test run CRUD, validation, conflicts, missing resources, the case-version capture each run records, recording, replacing and removing a result, JUnit XML and JSON result import, and listing, linking and unlinking the defect links a result carries |
| `tests/cases.rs` | Test case CRUD, required fields, parent-scoped creation, copy/move composition, conflicts, missing resources, and versioning — the `version`/`lastModified` stamp, the `revisions/` snapshots a qualifying update writes, and the history endpoints |
| `tests/milestones.rs` | Milestone CRUD, validation, conflicts, duplication, and progress derived from the referenced runs |
| `tests/configurations.rs` | Configuration CRUD, validation, conflicts, missing resources, restart persistence, and use by a run |
| `tests/metadata.rs` | The context-bar listings: the distinct, sorted release and environment names derived from the milestones and configurations in the projects the caller reaches, filtered rather than refused, the empty array an installation with neither answers, and the document that publishes both |
| `tests/reports.rs` | The reports: the coverage report (per-suite and total case counts, the project scope filter, the global scope) and the run summary (the status buckets, the pass rate, the summed durations, the intersecting project/milestone/configuration and date filters), with the error answers for an unknown and an unusable identifier |
| `tests/request_id.rs` | The request id: the minted `X-Request-Id` on a request that sends none, the verbatim echo of an inbound one, the replacement of an empty header, distinct ids per request, the `requestId` the error envelope carries, the header a plain-text rejection still carries, and the id the request span is given |
| `tests/observability.rs` | The request span every request opens (method, path, status, latency), the `/metrics` counters in the Prometheus text format, the audit line a mutation writes and the failure line a refused one writes, and the credentials, bodies and attachment contents the whole-process log capture never holds |
| `tests/attachments.rs` | Upload, download, delete, content types, removal with the parent test case |
| `tests/tags.rs` | The `tags` array on projects, suites, cases and runs, the shared `?tags=` OR filter, and the OpenAPI parameter it is published through |
| `tests/validation.rs` | Scalar type validation: wrong-typed fields rejected on create and update with the field named, valid and omitted fields accepted, and documents persisted before the change still readable |
| `tests/security_tests.rs` | Path traversal, symlink escape, hardlink escape, malformed JSON, repository-level leniency, and concurrent writers |
| `tests/route_coverage.rs` | What the router actually served: every operation in `openapi.json` must have been driven to a successful answer during the run. Reads the recording the shared harness writes and asserts nothing unless `scripts/coverage-check.sh` turns it on |

Shared request builders and assertions live in `tests/common/mod.rs`. Cargo compiles only top-level
files in `tests/` as test binaries, so a subdirectory module is shared across suites without running
as one itself.

`tests/service.rs` also carries a coverage table: one row per operation in `openapi.json`, each
naming the test that drives it and asserts its success. The `every_documented_operation_has_a_covering_test`
guard compares that table against the served document in both directions, so an operation added to the
schema without a covering test fails the build, as does a row left behind by a renamed or removed one.
A row may not name one of the shared role or malformation sweeps, which drive many routes but assert
only the status they must refuse with. Because the API's contract is what `openapi.json` declares,
this is the coverage the project enforces; line coverage is not measured.

The table on its own is a declaration — it proves that the test it names exists, not that the test
reaches the route. `scripts/coverage-check.sh` adds the observed half: the shared harness records the
registered template of every request the router serves, and `tests/route_coverage.rs` then requires a
successful answer for each operation in the document. An operation no test drove, or one that only
ever answered an error, fails the check by name. It attributes a success to the run as a whole rather
than to the row's own test — the recording carries no test identity — so the two halves are read
together. Run it instead of a bare `cargo test` when you want the full guarantee:

```sh
scripts/coverage-check.sh                  # record, then check (fills target/route-coverage/hits.tsv)
cargo test --all-targets --all-features    # the same suite run, recording only
```

A plain `cargo test` stays green while recording: the check reads the recording the suites write, and
libtest orders the test binaries arbitrarily, so recording and checking cannot be one pass. The script
truncates its recording first, so a stale file from an earlier run can never stand in for a run that
covered less.

Run everything, a single layer, or one suite:

```sh
cargo test --all-targets --all-features   # unit + integration
cargo test --lib                          # unit tests only
cargo test --test attachments             # a single API suite
```

The crate exposes a library target (`src/lib.rs`) alongside the binary so integration tests can
import `tucano_test::api` and drive the router directly, without binding a network port.

GitHub Actions runs workflow linting, formatting, Clippy, unit and integration tests, a release build,
dependency auditing, secret scanning, production container scanning, and CycloneDX SBOM generation.
Main-branch builds and SemVer tags publish numbered container artifacts to GHCR.

The `checks`, `audit` and `sbom` jobs run inside a `rust` container, whose filesystem is discarded
after every run. They mount a persistent Docker volume for `/usr/local/cargo/registry` and
`/usr/local/cargo/git`, so the crates.io downloads a run does not already hold are fetched once and
then reused, instead of every run depending on `static.crates.io` resolving. Only those two
subdirectories are cached: the image's own `cargo` and `rustc` binaries stay in place, so a newer
image is never shadowed by a stale cache.

