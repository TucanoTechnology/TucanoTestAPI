# Performance Baseline

This document records the Criterion benchmark baseline for the Tucano Test
API domain layer. The harness lives in `benches/crud.rs` and is introduced
by [#97](https://github.com/TucanoTechnology/TucanoTestAPI/issues/97); it
is the single source of truth for the measurements below, and the
comparisons that future storage or domain work is judged against.

The numbers here are **not targets** — they are a record of what the
current implementation costs on a known machine, so a change can be
compared against a fixed point instead of a moving average. A regression
only matters when the cause is understood: a noisier machine, a fuller
disk, and a slower filesystem can all push the same code into the red.

## How to reproduce

```sh
cargo bench --bench crud
```

The bench binary is built in release mode by Cargo and talks directly to
[`TestService`](../../src/domain/service/mod.rs) backed by a
[`FileRepository`](../../src/storage/fs/mod.rs) on a `TempDir`. There is
no HTTP server, no network, and no shared state between runs: each
workload uses its own isolated directory, so results are independent and
re-runnable.

Rerun after any change that touches the storage or domain layers and
compare the new numbers against this table before declaring a
regression. A second run of the same binary on the same machine is the
right first check: a workload that reproduces the previous measurement
has not regressed.

## Baseline table

Recorded on **AMD Ryzen 5 8600G**, 64 GiB DDR5, Linux 7.0.0 (ext4 on
NVMe), `cargo bench --bench crud` against commit
`baseline` (fill in after merge), release profile.

Each workload is a single measurement: the median of 100 samples taken
after a three-second warm-up. The range is the 95 % confidence
interval that Criterion reports.

| Workload                                   | Time (median) | Notes                                                                                      |
|--------------------------------------------|--------------:|--------------------------------------------------------------------------------------------|
| `crud_case_full_cycle`                     |      ~520 µs  | Create → read → update (starts a revision snapshot) → delete of one test case.             |
| `list_projects/all`                        |      ~103 µs  | List all projects after seeding 64 of them. Exercises the folder walk and marker reads.    |
| `list_projects/with_filter`                |      ~104 µs  | Same tree with a substring filter; the filter runs in-memory after the walk.               |
| `list_cases_in_suite`                      |      ~173 µs  | List the 64 cases stored inside one suite. One folder walk, no marker reads.               |
| `validate_large_case_payload`              |       ~66 µs  | Validate a 100-step, 1 000-tag test case without persisting it. Pure CPU, no I/O.          |
| `attachment_upload_delete`                 |      ~246 µs  | Store and delete an 8 KiB attachment against a persistent case. Atomic write + rename.     |

Bounded resources:

* **Seed cap**: `benches/crud.rs::SEED_CAP = 64`. Raise it for a stress
  run; the number above is what the baseline was recorded at.
* **Attachment size**: `benches/crud.rs::ATTACHMENT_BYTES = 8 KiB`.
  Well below the API's 50 MiB cap, so the bench stays inside the
  documented contract.

## What each workload measures

* **`crud_case_full_cycle`** — the write path the API takes for a test
  case, end to end. The update touches a qualifying field so a revision
  snapshot is written under `revisions/`, matching how production
  updates behave. The identifier rotates per iteration, so each cycle
  sees a fresh case and no leftover state leaks across samples.
* **`list_projects`** — the cost of walking the projects folder and
  sorting the results. The filter variant adds an in-memory substring
  match, which is the one case where the listing reads documents: it
  exercises the branch that the tags and configuration filters share.
* **`list_cases_in_suite`** — the cost of listing a single parent's
  children. The benchmark seeds the suite once and only measures the
  walk, so a change that slows folder enumeration shows up here.
* **`validate_large_case_payload`** — the payload validator on the
  upper tail of what the API accepts. A hundred structured steps and a
  thousand tags are well above any realistic case, so this is the cost
  of "a big case someone could submit", not the typical case.
* **`attachment_upload_delete`** — the atomic-write path the attachment
  routes take: same-directory temporary file, flush, rename, and the
  marker-document re-serialisation that records the metadata. The
  matching delete exercises the reverse path.

## How to read a regression

Criterion reports a `change` percentage against the last saved baseline
in `target/criterion/`. A flag of `Performance has regressed` means the
new measurement sits outside the previous confidence interval; the same
flag for an improvement is symmetric. Both are **signals to investigate**,
not verdicts:

1. Re-run the bench. A single noisy sample often flags; the second run
   usually does not.
2. Check the host. A concurrent compile, a CI worker, or a full disk
   will shift every workload uniformly.
3. Look at the outlier count. A jump from the typical 4–7 % to above
   15 % points at a noisy environment, not a code change.
4. Compare across workloads. A change that moves every workload by the
   same amount is almost always the environment; a change that moves
   only the workload the commit touched is worth reading.

When a real regression is identified, open a follow-up issue rather
than silently optimising inside the bench ticket: the harness's job is
to measure, not to fix.

## Future work

The harness is intentionally narrow. Additions that are worth a
follow-up issue:

* A composition workload (copy/move a suite across projects), once
  #66's placement semantics are stable.
* A run-import workload (JUnit / JSON) for the write-heavy path.
* A concurrent workload using Criterion's `async` feature, once the
  multi-replica lock story is in place.

Until then, this baseline is the point of comparison for anything that
touches the storage or domain layers.
