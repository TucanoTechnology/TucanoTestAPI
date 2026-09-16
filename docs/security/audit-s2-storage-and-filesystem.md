# Security Audit S2 — Storage and filesystem invariants (Issue #177)

Issue: [#177](https://github.com/TucanoTechnology/TucanoTestAPI/issues/177). Parent epic:
[#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166) — *Carry out security audit*.

This report runs surface **S2** of [audit-scope.md](audit-scope.md): *"Path confinement and symlink
handling, atomicity and durability of writes, the advisory lock, attachment and revision storage,
file permissions on documents and on the authentication store, and the on-disk tree as an integrity
boundary."* It carries findings only; nothing here is fixed. Remediation belongs to the tickets
[#180](https://github.com/TucanoTechnology/TucanoTestAPI/issues/180) raises.

> **Status of this file: CHECKPOINT, not a finished audit.** The sub-tasks named in *Not yet
> executed* below have not run. **Do not read this as the S2 report and do not merge it as one.**
> It exists so that measured evidence survives the end of an off-peak window; the report is
> complete only when every sub-task in [audit-design-176-178.md](audit-design-176-178.md) §"#177"
> §3 has a result and §6 below is filled in.

- **Affected revision (the pinned target):** `c5e99431389854368ab3a8e07003622f34dfdd21`
- **Method:** [audit-scope.md § 6](audit-scope.md#6-how-the-audit-tasks-run), steps 1–7.
- **Findings so far:** **one** Low (`F-177-1`). Severity calibration across the surface (§6) has not
  been completed, so this count is provisional.
- **Pass entries so far:** four, in the [Pass entries](#5-pass-entries) section.
- **Executed:** S2-1 (partial), S2-2, S2-4. **Not executed:** S2-3, S2-5, S2-6…S2-15.

## 1. Revision pinned

| Item | Value |
| --- | --- |
| Repository revision audited | `c5e99431389854368ab3a8e07003622f34dfdd21` |
| `origin/main` at audit time | `c5e99431389854368ab3a8e07003622f34dfdd21` (merge base, so the report's tree is the audited tree) |
| `Cargo.lock` SHA-256 | `4cddc67a699e87847a5a1f9d5cba12c3d32f6c80c83e1c11459fd970064f78ab` |
| Built image id (the audited artifact) | `sha256:8db4b338b31c36d55fa168962c5d9522466a959c5bb86512636c44e41a173`, tagged `tucano-test-audit-177:c5e9943` |
| Audit worktree | `/home/emanuelec/Documents/Github/Tucano-Test-eco`, branch `eco/177-storage-audit`, working tree clean at the seeded revision |
| Throwaway data directory | `/tmp/audit-177/data`, bind-mounted at `/data`; **host filesystem `tmpfs`** (host `/tmp` is a tmpfs mount) |
| Container's view of the same directory | `stat -f -c %T /data` → `tmpfs`; `stat -c '%a %n'` → `777 /data`, owner `root:root` |

Two facts in that table are load-bearing for every measurement below and are recorded rather than
hoped for:

- **The throwaway volume is on `tmpfs`, while a real deployment's `./data` is not.** The operator's
  long-lived deployment was inspected read-only for comparison and its `/data` reports
  `stat -f -c %T /data` → `ext2/ext3`. POSIX permission bits and symlink handling behave the same on
  both, so the *permission* results transfer; durability and `flock`-under-overlay results would not,
  and no such result is claimed in this checkpoint.
- **Authentication was off in the audited arm.** No `TUCANO_JWT_SECRET` was supplied, so the service
  ran in its default unauthenticated mode (see §2.1, deviation 6). Every request below was made
  without credentials. Where a finding's impact depends on that, the finding says so.

## 2. Throwaway target (step 2)

One scratch Compose project, `audit-177`, built from the pinned revision, plus the `docker run` arms
the sub-tasks needed. The operator's long-lived instance (Compose project `tucano-test`) was never a
target: `tucano-test-api-1` reported `Up 34 hours` with `0.0.0.0:3100->3000/tcp, [::]:3100->3000/tcp`,
running image `tucano-test-api:local` = `8f075af3c2e7` (container `.Image` `13f10c0e9208`) before,
during, and after every probe in this checkpoint.

```yaml
# /tmp/audit-177/compose.yml
services:
  api:
    build:
      context: /home/emanuelec/Documents/Github/Tucano-Test-eco
      dockerfile: Dockerfile
    image: tucano-test-audit-177:c5e9943
    environment:
      TUCANO_DATA_DIR: /data
      PORT: 3000
    ports:
      - "3320:3000"
    volumes:
      - /tmp/audit-177/data:/data
    read_only: true
    tmpfs:
      - /tmp
    security_opt:
      - no-new-privileges:true
```

### 2.1 Deviations from the shipped Compose file

The shipped `docker-compose.yml` differs in the following ways. Each is recorded because a reader
judging a reproduction needs to know which part of the setup is the project's and which is the
auditor's.

1. **Host port:** `3100` → `3320`. The shipped publish syntax is kept (`- "3320:3000"`, no host-address
   prefix), so the all-interfaces publish is preserved. F-177-1 does not depend on this.
2. **Build context:** `.` → the worktree path. The container contract (`/data`, `PORT=3000`) is
   untouched.
3. **Image tag:** `tucano-test-api:local` → `tucano-test-audit-177:c5e9943`. Deliberate: the audit
   rebuilds the image without ever retagging the name the operator's live container holds.
4. **Data directory:** `./data:/data` → `/tmp/audit-177/data:/data`, a directory this audit created
   and deletes (§7). It is never `./data` and never the operator's volume.
5. **Data directory mode `0777`.** The container process is uid 10001 (`tucano`), while the host user
   that created the directory is uid 1000, so a default `mkdir` mode would not be writable by the
   service. `0777` is therefore the auditor's provisioning step, not a property of the repository —
   **but see F-177-1, where the operator's real `./data` is itself `drwxrwxrwx`**, so this deviation
   matches the deployment rather than departing from it. The report states the mode of both, and
   F-177-1's impact paragraph does not rest on the auditor's own chmod.
6. **Authentication off.** No `TUCANO_JWT_SECRET`, no `TUCANO_AUTH_ENABLED`. The default
   configuration is intended to be audited this way (`audit-scope.md` §5's escalation rule turns on
   the *default* configuration), and every finding states the setting explicitly.
7. **A second arm, not a second Compose service.** The cross-principal arm is a one-shot
   `docker run --user 4242:4242 --entrypoint sh` against the same data directory, using the audited
   image (so the same `sh` availability as the service's own runtime).

## 3. Surface enumerated before probing (step 3 — S2-1, **partial**)

### 3.1 Enforcement points in `src/storage/layout.rs`

Line numbers verified at the pinned revision with
`grep -n "RESERVED_PROJECT_CHILDREN\|ROOT_DIRS\|fn folder_name\|fn folder_wire_id\|fn validate_document_id\|fn validate_component\|fn ensure_within\|fn resolve_existing_prefix\|fn set_private_permissions" src/storage/layout.rs`:

| Line | Item | Role |
| --- | --- | --- |
| `:23` | `RESERVED_PROJECT_CHILDREN = ["test_runs", "milestones", "configurations"]` | the reserved collection names inside a project folder |
| `:49` | `ROOT_DIRS` | the top-level directories under the data root |
| `:155` | `folder_name` | derives a directory name from an identifier |
| `:160` | `folder_wire_id` | the inverse mapping, for wire ids |
| `:236` | `validate_document_id` | identifier validation before any path is built |
| `:367` | `validate_component` | validates a single path component |
| `:391` | `ensure_within` | the confinement check |
| `:415` | `resolve_existing_prefix` | resolves an existing prefix against the root |
| `:451` | `set_private_permissions` | the permission helper (see F-177-1) |

### 3.2 The four `set_private_permissions` call sites

| Call site | Operation | Imported at |
| --- | --- | --- |
| `src/storage/fs.rs:169` | atomic document write | `src/storage/fs.rs:12` |
| `src/storage/fs.rs:605` | attachment write | `src/storage/fs.rs:12` |
| `src/storage/fs.rs:710` | revision write | `src/storage/fs.rs:12` |
| `src/auth/store.rs:405` | auth store write | `src/auth/store.rs:26` |

The repository's own assertion of the resulting mode is `src/storage/fs.rs:2376`:
`assert_eq!(mode, 0o666);`. The design document cites this line as `:2286`; at this revision it is
`:2376`. The assertion is unchanged in substance.

### 3.3 Not yet enumerated

The closed mutating-call-site table the sub-task requires (`grep -n "fs::rename\|fs::write\|File::create\|OpenOptions\|create_dir\|remove_dir\|remove_file\|sync_all" src/storage/fs.rs src/storage/layout.rs src/auth/store.rs`,
plus the `attachment_path` / `step_attachment_path` / `revision_dir` / `project_document_path` /
`project_collection_dir` builders) has **not** been run. §3 is therefore not the closed table the
design asks for, and a path built outside `layout.rs` would not yet have been caught.

## 4. Findings

### F-177-1: Apply restrictive permissions to stored documents instead of world-writable 0o666

- **Severity:** Low
- **In scope:** S2 — storage and filesystem invariants; trust boundary 3 (*Resource ID or filename →
  filesystem*) and trust boundary 4 (*Service → stored JSON*).
- **Where:** `src/storage/layout.rs:451` (`set_private_permissions`), which sets mode `0o666` at
  `src/storage/layout.rs:455`; a class of defects, so the representative site plus the enumeration:
  `src/storage/fs.rs:169` (document), `:605` (attachment), `:710` (revision), `src/auth/store.rs:405`
  (auth store).
- **Affected revision:** `c5e99431389854368ab3a8e07003622f34dfdd21`
- **Reproduction** — the mechanism, quoted from the audited revision:

  ```rust
  // src/storage/layout.rs:450-457
  /// Restrict a freshly created file so the host user can read and write it.
  pub fn set_private_permissions(file: &File) -> io::Result<()> {
      #[cfg(unix)]
      {
          use std::os::unix::fs::PermissionsExt;
          file.set_permissions(std::fs::Permissions::from_mode(0o666))?;
      }
      Ok(())
  }
  ```

  and the run, from a clean deployment of the pinned revision, with the scratch Compose file
  in §2 and authentication off:

  ```bash
  cd /tmp/audit-177
  docker compose -p audit-177 up -d --build
  B=http://127.0.0.1:3320

  # Seed one document, one case, one attachment, one revision, one suite, one run, one configuration.
  curl -s -X POST "$B/projects" -H 'content-type: application/json' \
    --data-binary '{"name":"Checkout"}' -o /dev/null            # -> {"id":"Checkout.json"}
  curl -s -X POST "$B/projects/Checkout.json/test_cases" -H 'content-type: application/json' \
    --data-binary '{"testCaseId":"TC-LOGIN-1","title":"t","expectedResult":"e"}' -o /dev/null
  curl -s -X PUT "$B/test_cases/TC-LOGIN-1" -H 'content-type: application/json' \
    --data-binary '{"title":"revision probe"}' -o /dev/null      # writes revisions/v1.json
  curl -s -X POST "$B/test_cases/TC-LOGIN-1/attachments" \
    -F 'file=@/tmp/audit-177/upload.txt' -o /dev/null

  # The modes the service actually created.
  docker run --rm --entrypoint sh -v /tmp/audit-177/data:/d tucano-test-audit-177:c5e9943 \
    -c "find /d -printf '%M %u:%g %s %p\n' | sort"
  ```

- **Observed.** Every file the service creates is mode **`0666`** (`-rw-rw-rw-`), owned by
  `tucano:tucano` (host view `uid 110000 gid 100998`, the rootless subuid mapping of container uid
  10001), and every directory is `0755`. The lock file is the sole exception, at `0644`. The full
  listing, trimmed of repeated identical rows only where noted:

  ```
  -rw-r--r-- tucano:tucano 0   /d/.tucano.lock
  -rw-rw-rw- tucano:tucano 130 /d/projects/Checkout/project.json
  -rw-rw-rw- tucano:tucano 133 /d/projects/Checkout/TC-LOGIN-1/test-case.json
  -rw-rw-rw- tucano:tucano 137 /d/projects/Checkout/TC-LOGIN-1/revisions/v1.json
  -rw-rw-rw- tucano:tucano 18  /d/projects/Checkout/TC-LOGIN-1/1789534805621580870-evidence.txt
  -rw-rw-rw- tucano:tucano 42  /d/projects/Checkout/milestones/M1.json
  -rw-rw-rw- tucano:tucano 52  /d/projects/Checkout/configurations/Chrome.json
  -rw-rw-rw- tucano:tucano 72  /d/projects/Checkout/Smoke/suite.json
  -rw-rw-rw- tucano:tucano 88  /d/projects/Checkout/test_runs/Audit run.json
  drwxr-xr-x tucano:tucano 180 /d/projects
  drwxr-xr-x tucano:tucano 340 /d/projects/Checkout
  drwxr-xr-x tucano:tucano 60  /d/projects/Checkout/TC-LOGIN-1
  drwxr-xr-x tucano:tucano 60  /d/projects/Checkout/TC-LOGIN-1/revisions
  drwxr-xr-x tucano:tucano 60  /d/auth
  drwxr-xr-x tucano:tucano 40  /d/auth/projects
  drwxrwxrwx root:root    100 /d
  ```

  The mode is not incidental, and not merely an artifact of the audit's own volume: the repository
  asserts it (`src/storage/fs.rs:2376`, `assert_eq!(mode, 0o666);`), and the same pattern is present
  in the operator's real deployment, where `/data` is `ext2/ext3` and its stored documents are
  likewise `-rw-rw-rw-`:

  ```
  drwxrwxrwx  7 emanuelec emanuelec 4096 /home/emanuelec/Documents/Github/Tucano-Test/data
  -rw-rw-rw- tucano:tucano 10278 /data/projects/PRJ-ECOMMERCE-STOREFRONT.json
  -rw-rw-rw- tucano:tucano  581 /data/test_cases/TC-PAY-002/test-case.json
  -rw-rw-rw- tucano:tucano  108 /data/test_cases/TC-CHECKOUT-001/1788912998042772317-checkout-trace.log
  ```

  The operational consequence, measured in the second arm rather than inferred from the mode bits: a
  process running as an unrelated uid can read **and rewrite** a stored document, while the lock file
  refuses the same principal a write.

  ```bash
  docker run --rm --user 4242:4242 --entrypoint sh -v /tmp/audit-177/data:/d \
    tucano-test-audit-177:c5e9943 -c '
      id
      echo "--- document (0666) ---"; cat /d/projects/Checkout/project.json
      printf "TAMPERED-BY-ANOTHER-PRINCIPAL" >> /d/projects/Checkout/project.json \
        && echo "write: allowed"
      echo "--- lock file (0644) ---"
      cat /d/.tucano.lock >/dev/null && echo "read: allowed"
      printf x >> /d/.tucano.lock 2>/dev/null && echo "write: allowed" || echo "write: denied"'
  ```

  ```
  uid=4242 gid=4242 groups=4242
  --- document (0666) ---
  {
    "description": "audit S2",
    "name": "Checkout",
    "projectId": "checkout",
    "testSuites": []
  }
  write: allowed
  --- lock file (0644) ---
  read: allowed
  write: denied
  sh: 1: cannot create /d/.tucano.lock: Permission denied
  ```

  **What this measurement does not cover, stated rather than implied:** the auth store file was *not*
  exercised. With authentication off, the service creates `/data/auth` and `/data/auth/projects` as
  empty directories and writes no store document, so `src/auth/store.rs:405`'s mode is credited by
  inspection of the call site and by the shared helper — not by an observed `0666` file. An arm with
  `TUCANO_JWT_SECRET` set and one seeded account is required before that call site may be described as
  measured.

- **Expected.** `AGENTS.md` (Storage Security) instructs: *"Apply restrictive file permissions and
  explicit overwrite behaviour"*, and trust boundary 5's required control names *"safe permissions"*.
  A stored document should be owner-only (`0600`) or owner+group (`0640`); the auth store in particular
  should not be world-readable. The code's own doc comment at `src/storage/layout.rs:450` says
  *"Restrict a freshly created file so the host user can read and write it"* — 0o666 does not restrict;
  it grants the read and write bit to every principal on the host. The defect is therefore both a
  violation of the invariant and a contradiction of the repository's written rule and of the function's
  own stated intent.

- **Impact.** Within the threat model as written, the impact is **bounded by a precondition the scope
  excludes**: `audit-scope.md` §3 rules out *"a volume an attacker can both read and write"* and
  `threat-model.md` records the matching known limitation — *"File encryption protects against
  accidents, not against an actor who can read the volume."* An attacker who can read and write the
  volume can already tamper with the installation, so the finding does not *create* that capability.
  What it does is allow a **weaker** position than the excluded precondition to reach the same
  tampering: read/write on the files does not require write access to the directory, ownership of it,
  or any membership of the `tucano` group, and it applies on a host where other local accounts exist
  even when the data directory itself is not world-writable. With authentication **off** (the state
  measured above), that principal can also read documents through the API without any permission on
  the volume at all — the file mode is the weaker of the two problems in that configuration, which is
  why this finding is not scored as the more serious one. With authentication **on**, the mode bits
  are what stands between another local principal and the stored JSON, and `0600` would close it.
  If the deployment is single-user and the data directory is not reachable by another principal, the
  practical impact is Limited.

- **Severity, both readings, as the design requires.** Impact axis: **Moderate** — tampered data
  inside the installation's own data set and, potentially, disclosure of stored JSON. The auth store's
  contents are not a reason to score Severe: it holds Argon2id PHC hashes and refresh-token SHA-256
  digests, which invariant 7 permits, and those are not usable credentials. Exploitability axis:
  **Difficult** — a local position on the host (or another local account), and a deployment
  configuration in which the data directory is reachable by that principal. *Difficult × Moderate =
  **Low***, and the stated de-escalation for the excluded precondition is already applied in arriving
  at Difficult. The design's alternative reading — **Medium**, if the executor demonstrates another
  local principal on a default deployment — is **not** taken here, and the reason is recorded rather
  than left implicit: the demonstration above is of *permission bits*, which is enough to prove the
  defect but not to establish that a deployment's data directory is reachable by a second principal,
  and the operator's own deployment is a single-user host. Nothing in this checkpoint shows a second
  local account able to reach the volume; if a later probe does, the reading moves to Medium.

- **Suggested fix.** Confine the mode to the owner in `set_private_permissions` — a restrictive mode
  such as `0600` for files and `0700` for directories — and make the mode explicit at each of the four
  call sites rather than implicit in the helper, so that a new call site cannot silently reintroduce
  the world-writable default. The repository's own assertion at `src/storage/fs.rs:2376` encodes
  0o666 and must be updated with the change; the audit recommends the test, and does not write it.

- **CWE:** CWE-276 (Incorrect Default Permissions); CWE-732 (Incorrect Permission Assignment for
  Critical Resource) is the closer fit for the auth store's file.

- **Duplicates / prerequisites:** none.

### Recorded observations (not findings)

**O-177-1 — A stray `.tucano-<suffix>.tmp` file is inert and unaddressable.** The atomic-write
mechanism leaves a temporary file behind when a write is interrupted before its `rename`. Naming one
directly is refused rather than served: `POST /projects/Checkout.json/test_cases` with
`{"testCaseId":".tucano-1700000000000000000.tmp", …}` is **accepted** as an ordinary *case* identifier
(HTTP 201 — the string is validated and used as a directory name in that position), and the resulting
path is a directory the service created, not the atomic-write temporary file, which lives one level
away as a sibling of the destination document. No route reads a `.tucano-*.tmp` file's contents. Per
the design's pre-commitment (§2.7) this is a pass condition, recorded here because the acceptance of
the name is a fact a later sub-task (S2-11's overwrite table) will need.

**O-177-2 — The lock file's mode differs between a fresh installation and the operator's existing
one.** A scratch deployment creates `/data/.tucano.lock` as `-rw-r--r--` (0644); the operator's
long-lived deployment carries `-rwxrwxrwx` (0777), which is what an earlier revision left behind. The
lock file holds no data of value, so this is recorded and not scored; it does show that a permission
change to the lock file would not propagate to an existing volume without a migration.

**O-177-3 — The startup-created directories under the auth root are empty when authentication is
off.** `/data/auth` (0755) and `/data/auth/projects` (0755) exist after startup with no store document
inside them. This is why F-177-1's auth-store call site is credited by inspection only, and it is the
starting state an authenticated arm must differ from.

**Pending triage — measured, not yet scored.** The following results were produced by the S2-4
identifier probes and are **candidates** whose severity has not been scored. They are recorded so the
measurement is not lost; they are not findings until scored and written in the §"Required finding
shape" form:

| Probe | Result |
| --- | --- |
| case id `test_runs`, `milestones`, `configurations` (reserved project children), in a freshly created project | **409** `conflict` — refused, and refused in a project whose collection directory does not pre-exist |
| case id `.tucano.lock` | **201** — accepted as a case directory name |
| case id `.tucano-1700000000000000000.tmp` | **201** — accepted (see O-177-1) |
| case id `CON`, `nul`, `aux` (Windows device names) | **201** — accepted; no refusal, so a Windows-hosted volume is the only place the name becomes special |
| case id `.`, `..`, `a/b`, `a\b` | **400** `invalid_request` — refused |
| case id `a%2Fb` | **201** — stored literally, no traversal |
| case id `""` | **400** `Required fields are missing` |
| case id `"   "` (whitespace only) | **201** — accepted, producing a whitespace-named directory |
| case id 4096 bytes long | **500** `storage_error` `"Storage operation failed"` — an uncontrolled failure class where the identifier-length check should have refused with a 4xx |
| case id `cafe\u0301` (combining accent) | **201** — accepted as its own distinct identifier |
| duplicate case id `TC-LOGIN-1` | **409** `conflict` |
| project names `test_runs`, `milestones`, `configurations`, `smoke`, `audit probe` | **201** — project-level names are not subject to the reserved-child rule, as expected |
| suite created with body `{"suiteId":"test_runs","name":"t"}` | **201** with `{"id":"t.json"}` — the suite id is derived from `name`, so this probe did **not** test a reserved suite name and must be re-run against the correct field |

The 4 KiB → 500 result is the most promising of these (an identifier that should be refused produces a
server error class instead); it needs a severity score and, if it holds, a finding in the required
shape with its own reproduction. The accepted whitespace-only and dotfile identifiers need a decision
against the design's §2.7 reasoning — dotfiles in particular, since `.tucano.lock` and
`.tucano-*.tmp` are names the storage layer itself uses.

## 5. Pass entries

Controls tested **and not broken** in this checkpoint:

1. **Traversal characters in an identifier are refused, not laundered.** `"."`, `".."`, `"a/b"`, and
   `"a\b"` as a case identifier all return **400** `invalid_request`; the request never reaches the
   filesystem layer. (Trust boundary 3.)
2. **A percent-encoded separator stays literal.** `a%2Fb` is accepted as the identifier `a%2Fb` and
   stored as the directory `a%2Fb`, with no decoding into a path separator. (Trust boundary 3.)
3. **Reserved project children are refused as case identifiers.** `test_runs`, `milestones`, and
   `configurations` each return **409** `conflict` — and the refusal is by validation, not by an
   accidental collision with a pre-existing directory, because it reproduces in a project created
   moments earlier (`probe2.json`, whose `test_runs` collection does not exist). This credits
   `src/storage/layout.rs:23`'s `RESERVED_PROJECT_CHILDREN` as an enforced control.
4. **A duplicate identifier is a conflict, not an overwrite.** Creating `TC-LOGIN-1` twice returns
   **409** `conflict` and leaves the stored document unchanged.

Not yet credited in this checkpoint (and deliberately not listed as passes): symlink and hardlink
escape fixtures (S2-3), atomicity under `SIGKILL` (S2-5), corrupted-document handling (S2-6),
concurrent writers (S2-7, S2-8), lock release (S2-9), attachment and revision publication (S2-10),
the overwrite table (S2-11), error-path disclosure (S2-12), the read/write confinement to the root
(S2-13), the auth-tree unreachability (S2-14), and the configuration-file boundary (S2-15). The
repository's own tests — `src/storage/layout.rs::a_symlink_that_escapes_the_root_is_rejected`,
`::a_symlinked_collection_directory_that_escapes_the_root_is_rejected`,
`::a_symlinked_project_folder_that_escapes_the_root_is_rejected`,
`tests/security_tests.rs::symlink_tests::test_rejects_symlink_escape`,
`tests/security_tests.rs::data_integrity_tests::test_concurrent_writes_do_not_corrupt` — are baselines per
`audit-scope.md`, not findings, and this audit has not yet re-run them. They are named here so the
next checkpoint's S2-3 and S2-7 either credit them with a fresh run or record the gap.

## 6. Calibration confirmed

**Not completed in this checkpoint.** The severity calibration across the surface — that each
finding's two axes were scored against the rubric's tests and not against intuition, and that the
provisional finding count matches §4 — is the step that closes an audit report and cannot be done
while sub-tasks are outstanding.

## 7. Tear-down (step 7)

**Recorded at the end of the window in a follow-up commit to this file.** At the time of writing this
checkpoint the scratch project `audit-177` and `/tmp/audit-177` are still live, because the volume
holds the seeded tree the *Not yet executed* sub-tasks will reuse. The tear-down entry will record:
`docker compose -p audit-177 down -v`, removal of the container and of the
`tucano-test-audit-177:c5e9943` image, removal of `/tmp/audit-177` (including whether the uid-10001
ownership workaround was needed, as it was for S3), and the operator-instance check that
`tucano-test-api-1` is still `Up` on `tucano-test-api:local` = `8f075af3c2e7`.

## Not yet executed in this checkpoint

The following sub-tasks of [audit-design-176-178.md](audit-design-176-178.md) §"#177" §3 have not run
at all. Each names what it is for, so a reader can see the shape of what is missing rather than only
its absence.

| Sub-task | What is missing |
| --- | --- |
| **S2-1 (remainder)** | The closed mutating-call-site table required by report §3 (see §3.3). |
| **S2-3** | The six symlink and hardlink escape fixtures planted inside the throwaway tree: `a_symlink_that_escapes_the_root_is_rejected`, `a_symlinked_collection_directory_that_escapes_the_root_is_rejected`, `a_symlinked_project_folder_that_escapes_the_root_is_rejected`, `tests/security_tests.rs::symlink_tests::test_rejects_symlink_escape`, plus the hardlink fixture the DoD explicitly requires, and a symlinked *attachment* — each expected to be refused, with a bypass de-escalating one level per the design. |
| **S2-4 (remainder)** | The reserved-suite-name probe re-run against the correct body field (`name`), and the scoring of the pending-triage table in §4. |
| **S2-5** | Atomicity: ten `SIGKILL`s of the container process mid-write, then a JSON validation pass over every stored document and an inspection of leftover `.tucano-*.tmp` files. No power-loss durability is claimed either way; a missing parent-directory `fsync` is an observation by pre-commitment, never a finding. |
| **S2-6** | Corrupted and hostile stored documents: truncated, invalid UTF-8, wrong-shape JSON, and a 100 MiB replacement — expecting a safe `500 storage_error` that preserves the corrupted bytes and discloses no path, stack, or file content. |
| **S2-7** | 32 concurrent writers to one document inside one replica. |
| **S2-8** | Two replicas against one data directory, with the filesystem type of the throwaway volume recorded — note that this checkpoint's volume is `tmpfs`, so this sub-task's result does **not** transfer to a real volume and the arm must be re-provisioned on a disk-backed directory before its result may be written up. |
| **S2-9** | Lock release on the failure path: a write that fails after the lock is acquired, then a normal write, for the document, attachment, and revision paths. |
| **S2-10** | Attachment publication in place (torn read), orphan handling, and revision immutability. |
| **S2-11** | The overwrite-contract table for every mutating operation, including the two imports whose conflict behaviour the design says is measured rather than assumed. |
| **S2-12** | The error-leak table across every `DomainError` variant and every layer — the DoD item. |
| **S2-13** | The root-is-the-only-area-read-or-written probe: a filesystem hash of the container outside `/data` and `/tmp` before and after a full workload, plus `docker diff`. |
| **S2-14** | Confirmation that no route lists, reads, or writes anything under `TUCANO_DATA_DIR/auth/`. |
| **S2-15** | The storage side of the configuration-file boundary, including the check of whether `#189`'s AEAD envelope has landed at the audited revision (which decides whether the *key* boundary is exercised or recorded as documented-pending). |

Also outstanding for the finished report: the README documentation-table row, the full local gate
(`actionlint`, `node scripts/check-matrix.mjs`, `cargo fmt --check`, `cargo clippy`, `cargo test`,
`cargo build --release`), and the pull request itself — which per the design is opened **only** when
the report is complete, assigned to `ECiurleo` and never merged by the auditor.
