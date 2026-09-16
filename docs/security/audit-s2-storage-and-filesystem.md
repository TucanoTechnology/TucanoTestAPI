# Security Audit S2 — Storage and filesystem invariants (Issue #177)

Issue: [#177](https://github.com/TucanoTechnology/TucanoTestAPI/issues/177). Parent epic:
[#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166) — *Carry out security audit*.

This report runs surface **S2** of [audit-scope.md](audit-scope.md): *"Path confinement and symlink
handling, atomicity and durability of writes, the advisory lock, attachment and revision storage,
file permissions on documents and on the authentication store, and the on-disk tree as an integrity
boundary."* It carries findings only; nothing here is fixed. Remediation belongs to the tickets
[#180](https://github.com/TucanoTechnology/TucanoTestAPI/issues/180) raises.

> **Status of this file: CHECKPOINT, not a finished audit.** The sub-tasks named in *Not yet
> executed* below have not all run. **Do not read this as the S2 report and do not merge it as one.**
> It exists so that measured evidence survives the end of an off-peak window; the report is
> complete only when every sub-task in [audit-design-176-178.md](audit-design-176-178.md) §"#177"
> §3 has a result and §6 below is filled in.

- **Affected revision (the pinned target):** `c5e99431389854368ab3a8e07003622f34dfdd21`
- **Method:** [audit-scope.md § 6](audit-scope.md#6-how-the-audit-tasks-run), steps 1–7.
- **Findings so far:** **four**, all Low (`F-177-1`, `F-177-2`, `F-177-3`, `F-177-4`). Severity
  calibration across the surface (§6) has not been completed, so this count is provisional.
- **Pass entries so far:** thirteen, in the [Pass entries](#5-pass-entries) section.
- **Executed:** S2-1 (partial), S2-2, S2-3 (partial), S2-4 (partial), S2-6 (partial), S2-7
  (partial), S2-9 (partial), S2-13, S2-14 (partial). **Not executed:** S2-5, S2-8, S2-10, S2-11,
  S2-12 (partial), S2-15.

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

### F-177-2: Validate the shape of a stored document before serving it

- **Severity:** Low
- **In scope:** S2 — storage and filesystem invariants; trust boundary 4 (*Service → stored JSON*).
- **Where:** the document read path of the case service, `src/domain/service.rs`'s read of a stored
  case document (the same read path the corruption probe exercises). The defect is in the read, not
  in the write: the stored bytes are returned to the client without a shape check against the
  document type the route declares in `openapi.json`.
- **Affected revision:** `c5e99431389854368ab3a8e07003622f34dfdd21`
- **Reproduction** — from a clean seeded deployment (the §2 scratch target, authentication off):

  ```bash
  # Replace a stored case document with valid JSON of the wrong shape.
  docker run --rm --entrypoint sh -v /tmp/audit-177/data:/d tucano-test-audit-177:c5e9943 -c '
    printf "[1,2,3]" > /d/projects/Checkout/TC-COPY-1/test-case.json
    sha256sum /d/projects/Checkout/TC-COPY-1/test-case.json'

  curl -s -w '\n%{http_code}\n' http://127.0.0.1:3320/test_cases/TC-COPY-1
  ```

- **Observed.** **HTTP 200** with the hostile payload as the response body, echoed verbatim:

  ```
  [1,2,3]
  200
  ```

  The stored file's SHA-256 is unchanged by the read
  (`a615eeaee21de5179de080de8c3052c8da901138406ba71c38c032845f7d54f4` before and after, size 7), so
  the bytes are served as-is rather than reinterpreted. `openapi.json` declares this route's 200
  schema as the case-document object, not an array. By contrast, the three *malformed* variants of
  the same probe (`truncate`, invalid UTF-8, a 100 MiB blob) are each refused with
  `500 storage_error` and leave the file byte-identical — those are pass entries (§5), and they show
  the read path does detect *unparseable* documents. It is the *parseable but wrong-shaped* document
  that is passed through unfiltered.

- **Expected.** The read path should refuse a stored document that does not deserialize into the
  document type the route declares — the design anticipated exactly this and pre-committed to the
  expected outcome: *"A **safe storage error** (500 `storage_error` with a stable, non-disclosing
  message) and **the original file is preserved**"* ([audit-design-176-178.md](audit-design-176-178.md),
  S2-6). Preserving the bytes is observed; refusing the document is not. Trust boundary 4 treats the
  stored JSON as data crossing a boundary, and the audit's acceptance criterion is that a violation
  reachable in a deployed configuration is a finding whatever the code intends.

- **Impact.** An actor with write access to the volume can dictate the body the API returns from any
  case route, with a 200 status and no schema check, to every client of the installation — the GUI
  included. The XML/JSON consumer that trusts the declared schema receives a type it did not expect
  (an array where an object is declared). The effect is bounded by the same excluded precondition as
  F-177-1 (write access to the volume), and it discloses nothing the actor did not already possess;
  what it defeats is the API's own statement about the shape of what it returns, which is why it is
  scored rather than recorded. With authentication **off** the modified body is served to any caller
  of the unauthenticated API; with authentication **on** it is served to any authenticated client.

- **Severity.** Impact axis: **Moderate** — tampered data served to a party other than the tamperer,
  plus the defeat of one control (the response schema) that by itself grants nothing further.
  Exploitability axis: **Difficult** — write access to the volume, a precondition `audit-scope.md` §3
  excludes, and therefore already the reason the axis is not Trivial or Moderate.
  *Difficult × Moderate = **Low***. No escalation applies: the defect is not reachable remotely in a
  default configuration without the volume position.

- **Suggested fix.** Confine the read to a shape check: deserialize the stored bytes into the
  document type at the storage boundary and let a mismatch become the same safe `500 storage_error`
  the malformed variants already produce, rather than returning the raw `serde_json::Value`. A
  regression test belongs next to `tests/security_tests.rs::malformed_json_tests`, which covers the
  unparseable case and not this one — the audit recommends it and does not write it.

- **CWE:** CWE-502 is not the fit (no deserialization of untrusted types into code); CWE-1287
  (Improper Validation of Specified Type of Input) and CWE-20 (Improper Input Validation) fit the
  read-side shape gap.

- **Duplicates / prerequisites:** shares its probe with S2-6 and its precondition with F-177-1; it is
  not a duplicate of either — F-177-1 is about the mode bits of the file, this is about the content
  the API is willing to serve from it.

### F-177-3: Concurrent writes are acknowledged with `200` and then silently discarded

- **Severity:** **Low** — Difficult × Moderate. The trigger is a race, which the rubric places on the
  Difficult row, and the effect is loss of data inside a scope the writer is already authorised to
  write. The default-configuration escalation was considered and **not** applied: it would apply to
  any authenticated client with two open requests, but no attacker gains anything — the loss is
  symmetric among the writers who are entitled to the resource, and the report chose the reading that
  the race precondition already carries the weight. The Medium reading (Trivial × Moderate, or Low
  escalated one level) is arguable and a later reader may take it; the evidence below is what matters.
- **In scope:** S2-7; trust boundary 4 (service → stored JSON); invariant 2 — the surviving document
  is always complete and always valid, but a write the API *acknowledged* can be absent from it.
- **Where:** the document write path (`src/domain/service.rs` → the storage layer's read-modify-write
  for a case document) and the advisory lock at `src/storage/fs.rs:36` with its eight unlock sites.
  The audit measured the behaviour and did **not** localise the exact window this checkpoint, so the
  mechanism below is inferred from the two experiments and said so.
- **Affected revision:** `c5e99431389854368ab3a8e07003622f34dfdd21` (`tucano-test-audit-177:c5e9943`).
- **Reproduction.** Against the throwaway stack in §2 (authentication off, host port 3320):
  1. `POST /projects {"name":"S27"}` → `201`; `POST /projects/S27.json/test_cases
     {"testCaseId":"TC-S27","title":"S27 base","expectedResult":"ok"}` → `201`.
  2. Fire 32 concurrent writes with distinct bodies:
     `for i in $(seq 1 32); do curl -s -o /dev/null -w '%{http_code}' -X PUT
     http://127.0.0.1:3320/test_cases/TC-S27 -H 'content-type: application/json'
     --data-binary "{\"title\":\"S27 writer $i\",\"expectedResult\":\"ok\"}" & done; wait`
  3. `GET /test_cases/TC-S27` and `GET /test_cases/TC-S27/history`.
  4. Control: repeat the same 32 requests **sequentially** in a fresh project (`S27seq`).
- **Observed.** Concurrent run: **all 32 requests returned `200`**, and the stored document reports
  `"version": 17` with **16** history entries and **16** files in `revisions/`. Sequential control:
  **all 20 requests returned `200`** with `"version": 21`, **20** history entries and **20**
  revisions. The concurrent run therefore acknowledged twice as many writes as it persisted. A
  tight reproduction — ten rounds of exactly two concurrent writers — is fully deterministic:
  `20` acknowledged writes, version `2 → 12` (delta `1` per round), **11** revisions on disk, and the
  surviving titles in `revisions/` show one writer per round, e.g. round 4 kept writer A and silently
  dropped writer B. No document was ever left malformed, no partial file appeared, and the failed
  writes of the over-long-identifier probe (F-177-4) left nothing behind.
- **Expected.** Either a write the service acknowledges is durable — 32 accepted writes produce 32
  versions — or a write that will not be applied is refused with a `409` conflict, as duplicate
  creation already is. An accepted `200` that leaves no trace is a false success signal, and a client
  cannot tell which of its two concurrent edits survived.
- **Impact.** Silent, non-recoverable loss of a collaborator's or of the same client's just-accepted
  edit. `changedFields` history loses the entry too, so the loss leaves no audit trail. Confined to
  the writers' own authorisation scope, so Moderate rather than Severe. The overwrite/locking
  semantics are still listed as an **open decision** in `threat-model.md` ("Locking implementation
  and overwrite/conflict semantics"), which is why the design asks for this to be *measured*; the
  measurement now exists and the decision can be made against it.
- **Suggested fix.** Hold the advisory lock across the read-modify-write, or make the write
  conditional on the version the client read and return `409` when it has moved. A regression test
  belongs beside `tests/security_tests.rs::data_integrity_tests::test_concurrent_writes_do_not_corrupt`,
  which checks that the *document stays valid* and does not check that *every acknowledged write
  landed* — that gap is why this finding was not caught by the baseline. The audit recommends the
  test and does not write it.
- **CWE:** CWE-362 (Concurrent Execution Using Shared Resource with Improper Synchronization);
  CWE-367 (TOCTOU) is the related read-modify-write pattern.
- **Duplicates / prerequisites:** shares its probe with S2-7 (pass entry 13 records what did *not*
  break: no corruption, no partial document). Not a duplicate of F-177-1/F-177-2. Requires two
  concurrent clients; in a default deployment that means a valid account with write access.

### F-177-4: An identifier longer than the filesystem's name limit is accepted and then fails as `500 storage_error`

- **Severity:** **Low** — Moderate × Limited. Triggering it needs an account (in the shipped
  configuration, authentication is on), so not Trivial; the effect is confined to the caller's own
  request, so Limited.
- **In scope:** trust boundary 3 (resource ID → filesystem); invariant 3 (client-visible errors use
  stable codes and safe messages) — the code is stable and the message is safe, but a bad *input*
  produces a server-error class, which is what invariant 3 exists to prevent.
- **Where:** `validate_document_id` / `validate_component` (`src/storage/layout.rs:236`, `:367`) —
  neither bounds the length — with the resulting `ENAMETOOLONG` surfacing through the storage layer
  as `DomainError::Storage`.
- **Affected revision:** `c5e99431389854368ab3a8e07003622f34dfdd21` (`tucano-test-audit-177:c5e9943`).
- **Reproduction.** Create any project, then post a case whose `testCaseId` is `N` bytes of `B`:
  `for n in 254 255 256 300; do id=$(printf 'B%.0s' $(seq 1 $n)); curl -s -o /dev/null -w '%{http_code}\n'
  -X POST http://127.0.0.1:3320/projects/S24fresh.json/test_cases -H 'content-type: application/json'
  --data-binary "{\"testCaseId\":\"$id\",\"title\":\"t\",\"expectedResult\":\"r\"}"; done`
- **Observed.** `254` → **201**, `255` → **201**, `256` → **500**
  `{"code":"storage_error","message":"Storage operation failed"}`, `300` → **500**. The boundary is
  exactly the filesystem's `NAME_MAX`, so the identifier is passed to the filesystem unvalidated and
  the host limit becomes the API's limit. The same shape holds for a suite **name** of 4096 bytes
  (**500**). No partial directory was left behind (nothing longer than 255 bytes exists on the
  volume), the project still accepted a normal write (`AFTER-500` → `201`), and no lock was left
  held — so this is an error-class defect, not a corruption one.
- **Expected.** A refusal in the 4xx class with an `invalid_request`-style code, before the name
  reaches the filesystem, so that the same request behaves identically on every volume. `255` is also
  a *successful* identifier, which means the limit is host-dependent: a deployment on a filesystem
  with a different `NAME_MAX` would accept a different set of identifiers.
- **Impact.** Availability/error-class only, confined to the caller's own request: an authenticated
  client can produce `500`s at will, which pollutes monitoring and, more importantly, makes the
  boundary between "your input is wrong" and "the service is broken" invisible — the same
  undifferentiated message the corruption variants produce (O-177-5).
- **Suggested fix.** Bound the identifier length in `validate_document_id` /
  `validate_component` at the point where the other reserved-name rules live, and refuse with the
  existing `invalid_request` code. The audit recommends the change and does not make it.
- **CWE:** CWE-20 (Improper Input Validation); CWE-1287 is the related "improper validation of
  specified type of input" framing.
- **Duplicates / prerequisites:** not a duplicate of F-177-2 (content shape vs. identifier length) or
  of F-177-1 (file mode). This is the 4 KiB row of the pending-triage table below, now scored and
  promoted to a finding; the reproduction above supersedes that table's entry.

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

**O-177-4 — A symlinked collection directory inside a project is refused, but as a `storage_error`
rather than a controlled refusal.** Planting `test_runs -> /tmp` inside a project folder and posting a
run to it yields `500 {"code":"storage_error","message":"Storage operation failed"}`. The control
holds — nothing was written into `/tmp`, the symlink was left in place, and the container's `/tmp`
listed empty afterwards — so this is not an escape and not a finding. It is recorded because the
refusal surfaces as a server-error class, the same shape the 4 KiB identifier and the read-only
directory probes produce (O-177-6), and because a client cannot distinguish "the volume is hostile"
from "the service is broken". Same family as the pending-triage 4 KiB row.

**O-177-5 — Every corrupted-document variant is refused with one undifferentiated `storage_error`.**
Truncation, invalid UTF-8, and a 100 MiB replacement all produce
`500 {"code":"storage_error","message":"Storage operation failed"}` — a stable, non-disclosing message
with no path, no OS error, no stack trace, and no file content, which is what invariant 6 and
boundary 8 require (pass entries 7–9). The observation is the granularity: three distinct causes share
one message, so an operator cannot tell corruption from a permissions problem from an oversize
document in the logs by the response alone. Recorded, not scored.

**O-177-6 — The audited container restarted during the window, which bounds what `docker diff` can
show.** The S2-4 probes ran against a container whose uptime reset partway through, so the writable
layer was re-created from the image and `docker diff audit-177-api-1` reports **no changes at all** —
consistent with "nothing outside the mounts is written", but only for the current incarnation, and it
cannot distinguish "never wrote" from "wrote, then restarted". S2-13 therefore requires a controlled
before/after filesystem hash and `docker diff` on a container with a known, unbroken uptime; the empty
`docker diff` in this checkpoint is corroboration, not the probe. The read-only-directory probes
(S2-9) show that the write paths that can fail do fail cleanly, which is the part of writable-layer
behaviour this checkpoint *can* speak to.

**O-177-7 — The S2-9 probe was corrected mid-run, and the first attempt is recorded because it is
informative.** The first S2-9 attempt removed write permission from the *project* folder
(`chmod 0555 /data/projects/Checkout`) and the write **succeeded** (HTTP 200). That is correct
behaviour, not a defect: a case document is written into the case folder, and the project folder's own
mode is irrelevant to that write. The probe was re-run against the case folder and produced the
expected refusal (§5, pass entries 10–11). The incident is recorded because it is the reason the report
names the write target explicitly in each lock probe.

**O-177-8 — There is no `/attachments/<filename>` route; an attachment is reached through its case.**
`GET /attachments/<stored-filename>` returns **404** with an empty body, while the same filename
appears in `GET /test_cases/<id>` under `attachments[]` and can be deleted through the case
(`DELETE /test_cases/<id>/attachments/<filename>` → `200`). Recorded because S2-10 asks about
attachment publication and a reader will look for a direct download route; the 404 also shows the
router-level fallback produces an empty body rather than the JSON error shape used everywhere else
(only `/no-such-route-213` and `GET /attachments/..%2F..%2Fetc%2Fpasswd` behave this way).

**O-177-9 — A run or configuration may be named after a reserved collection because it nests inside
it.** `POST /projects/S24fresh.json/test_runs {"name":"test_runs"}` → **201**, stored at
`projects/S24fresh/test_runs/test_runs.json`, and `POST .../configurations {"name":"test_runs"}` →
**201** at `projects/S24fresh/configurations/test_runs.json`. The reserved-child rule that refuses
`test_runs` as a *suite* name (and as a case id) is therefore about the project's own child
directories only; a run or configuration identifier is one level deeper and cannot collide. Recorded
so the asymmetry — refused at one level, accepted at the next — is not later mistaken for an
inconsistency.

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
| case id 4096 bytes long | **500** `storage_error` `"Storage operation failed"` — **scored and promoted to `F-177-4`**; the boundary is `NAME_MAX` (`255` → 201, `256` → 500) |
| case id `cafe\u0301` (combining accent) | **201** — accepted as its own distinct identifier |
| duplicate case id `TC-LOGIN-1` | **409** `conflict` |
| project names `test_runs`, `milestones`, `configurations`, `smoke`, `audit probe` | **201** — project-level names are not subject to the reserved-child rule, as expected |
| suite created with body `{"suiteId":"test_runs","name":"t"}` (the invalid probe) | **201** with `{"id":"t.json"}` — the suite id is derived from `name`, so that probe did **not** test a reserved suite name |
| suite name `test_runs`, `milestones`, `configurations` in a **freshly created** project, re-run against `name` | **409** `conflict` — refused by validation, not by a pre-existing directory; the control `Smoke` → **201** and a repeat `Smoke` → **409** `conflict` |
| run and configuration names `test_runs` in the same fresh project | **201** each — recorded as O-177-9 (one level deeper, no collision) |

Two rows are now closed by this checkpoint: the 4 KiB row became **`F-177-4`**, and the invalid
suite probe was re-run against the correct field and confirms the reserved-child rule for suites. The
accepted whitespace-only and dotfile identifiers still need a decision against the design's §2.7
reasoning — dotfiles in particular, since `.tucano.lock` and `.tucano-*.tmp` are names the storage
layer itself uses.

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
5. **Symlink fixtures planted inside the tree are refused, and nothing follows the link.** A case
   folder replaced by a symlink to a file outside the tree is refused on both **GET** and **PUT**
   (**404**, no body); a reserved collection directory replaced by `test_runs -> /tmp` is refused
   with **500** `storage_error`, the symlink is left in place, and the container's `/tmp` was
   verified **empty** afterwards. The second refusal is recorded for its error class in O-177-4;
   the control itself holds — no write landed outside `/data`. (Trust boundary 3.)
6. **A hardlink inside the tree is not written through.** With a second hardlink to a case document
   planted in the tree, a write through the service replaced the **directory entry** (atomic temp +
   rename) rather than following the link: the other link's inode (`120396`), its link count (`3`),
   and its content (the hand-written canary, unchanged) all survived, and the attempt returned **500**
   `storage_error` `"Stored JSON is invalid"`. A hardlink is therefore not a write-through path into
   or out of the document store. (Trust boundary 3; the DoD's hardlink item.)
7. **A truncated document is refused, byte-for-byte.** Replacing a stored document with 20 bytes of
   valid-JSON prefix yields **500** `storage_error`; afterwards the file's length is still `20` and
   its sha256 is still `cfc187ab0ba90ae84aadf438241756e837d73c78ac9e7dfd6fa9b3bdf7189dd1` — the
   service neither repaired nor rewrote the corrupted bytes, and the response disclosed no path, OS
   error, or stack. (Invariant 6; boundary 8.)
8. **A document that is not valid UTF-8 is refused, byte-for-byte.** **500** `storage_error`, length
   still `24`, sha256 still `190969eec63eea2cc4a9934ebbb705c3ad8e6ea4f6e5d535380eae3c0c1adc73`, and
   the same non-disclosing body. (Invariant 6; boundary 8.)
9. **An oversize document is refused, byte-for-byte.** A 100 MiB replacement (sha256
   `cee41e98d0a6ad65cc0ec77a2ba50bf26d64dc9007f7f1c7d7df68b8b71291a6`, size `104857600`) is
   refused with **500** `storage_error` and the bytes are left in place. No file content, path, or
   limit value appears in the response. (Invariant 6; boundary 8.)
10. **The document lock is released when the write fails.** With the case folder set to `0555` the
    document write returned **500**; restoring `0755` and re-issuing the same request returned **200**
    `{"message":"Resource updated"}`, i.e. no lock was left held by the failed attempt. No leftover
    `.tucano-*` entry was found in the tree; the only such name present was the S2-4 fixture
    *directory*. (Invariant 9's release half; see O-177-7 for the probe's correction.)
11. **The attachment lock is released when the write fails.** With the case folder at `0555` the
    attachment POST returned **500**; after restoring `0755` the same POST returned **201** with a
    stored `filename`. Both failure paths therefore unlock. (Invariant 9; S2-10's lock half only —
    torn reads and orphans are still owed.)
12. **Nothing outside `/data` and `/tmp` is read or written.** A full mutating workload was run —
    project, suite, case create; case update and duplicate; history read; run and configuration
    create; attachment upload and delete; a case delete and a project delete; plus a 404 and a 400 —
    on a container whose start time was identical for both measurements (`04:59:10Z`, unbroken
    uptime), and measured before and after: a `find / -xdev` + `stat` digest of everything outside
    `/data` and `/tmp` is **byte-identical**
    (`f283060083e00446f2e6ffd42a3e1fb511cb79cd867e70e9fb91d67f8b7dc156`), and `docker diff` lists
    **zero** changes (empty before and after; the empty-set sha256 is `e3b0c442…`). The image layer
    is mounted read-only (`overlay … ro`) and the workload's footprint is confined to the two mounts.
    Caveat recorded: the digest covers what uid `10001` can read — `find` could not descend into
    `/etc/ssl/private`, `/var/cache/apt/archives/partial`, `/var/cache/ldconfig`, or `/root` — which
    is why `docker diff`, taken host-side with full privileges, is included as the complete check.
    (Invariant 1.)
13. **Concurrent writers never corrupt the document or leave debris.** After the 32-writer storm of
    `F-177-3` the stored document is complete, parseable JSON served with `200`; the atomic
    temp-and-rename left **no** `.tucano-*` file anywhere under `/data` (the only such name in the
    tree is the S2-4 fixture *directory*); and a collision is refused rather than merged (`409`).
    What did **not** hold is the durability of every acknowledged write — that is `F-177-3`, and this
    entry is deliberately limited to what passed. (Invariant 2's atomicity half; boundary 4.)

Outside the numbered entries, S2-14 found the same shape on the auth tree: `/auth`, `/auth//`,
`/data/auth`, `/auth/projects`, and `/projects/../auth` all return **404**, and `/auth/me` returns
**401** without a token — no route lists, reads, or writes the store under `TUCANO_DATA_DIR/auth/`.
That is recorded here rather than as an entry because it is a `partial` sub-task: the authenticated
arm (with an auth store actually created) has not been probed. (Boundary 6/7.)

Not yet credited in this checkpoint (and deliberately not listed as passes): atomicity under `SIGKILL`
(S2-5), two replicas on one data directory (S2-8), attachment and revision publication (S2-10), the
overwrite table (S2-11), the full error-leak table (S2-12), and the configuration-file boundary
(S2-15). The
repository's own tests — `src/storage/layout.rs::a_symlink_that_escapes_the_root_is_rejected`,
`::a_symlinked_collection_directory_that_escapes_the_root_is_rejected`,
`::a_symlinked_project_folder_that_escapes_the_root_is_rejected`,
`tests/security_tests.rs::symlink_tests::test_rejects_symlink_escape`,
`tests/security_tests.rs::data_integrity_tests::test_concurrent_writes_do_not_corrupt` — are baselines per
`audit-scope.md`, not findings, and this audit has not yet re-run them. The three symlink baselines
correspond to the fixtures measured in entries 5–6 above. The concurrency baseline is still owed a
fresh run: S2-7's evidence is the API-level measurement recorded in `F-177-3` and pass entry 13, and
`F-177-3` names that baseline — which asserts only that the document stays valid — as the place a
durability regression test belongs.

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
to completion. Each names what it is for, so a reader can see the shape of what is missing rather
than only its absence. Rows marked **partial** have measured results in §4/§5; what they still owe is
in the second column. S2-3, S2-4, S2-7 and S2-9 have run; their rows are kept only to name what the
run did **not** cover (S2-13 has run in full and its row is gone).

| Sub-task | What is missing |
| --- | --- |
| **S2-1 (remainder)** | The closed mutating-call-site table required by report §3 (see §3.3). |
| **S2-3 (partial)** | The six fixtures were planted and refused (pass entries 5–6, O-177-4), but a **symlinked attachment** was not planted, and the repository's own symlink baselines (`src/storage/layout.rs::a_symlink_that_escapes_the_root_is_rejected`, `::a_symlinked_collection_directory_that_escapes_the_root_is_rejected`, `::a_symlinked_project_folder_that_escapes_the_root_is_rejected`, `tests/security_tests.rs::symlink_tests::test_rejects_symlink_escape`) were not re-run. |
| **S2-4 (partial)** | The reserved-suite-name probe was re-run against the correct body field: in a freshly created project `name: test_runs` → **409** `conflict` (refused by validation, not by a pre-existing directory), control `Smoke` → **201** and a repeat → **409**; the 4 KiB row is scored and promoted to `F-177-4` (the boundary is `NAME_MAX`: 255 → 201, 256 → 500). What remains is the decision on the two unrefused degenerate identifiers — whitespace-only and the dotfile names (`.tucano.lock`, `.tucano-<suffix>.tmp`, accepted as case ids → 201). |
| **S2-5** | Atomicity: ten `SIGKILL`s of the container process mid-write, then a JSON validation pass over every stored document and an inspection of leftover `.tucano-*.tmp` files. No power-loss durability is claimed either way; a missing parent-directory `fsync` is an observation by pre-commitment, never a finding. |
| **S2-6 (partial)** | Truncation, invalid UTF-8, and a 100 MiB replacement are measured (pass entries 7–9). The wrong-shape JSON case is measured **and is a finding** instead of a pass (`F-177-2`). No further variants are owed, but `F-177-2` needs the calibration pass in §6. |
| **S2-7 (partial)** | Measured: 32 concurrent PUTs of one document all returned **200** but only 16 persisted (`version: 17`), against a sequential control of 20 × 200 → 20 persisted (`version: 21`), and a 2-writer × 10-round reproduction where all 20 acknowledged writes yielded one new version per round. Written up as `F-177-3`, with `tests/security_tests.rs::data_integrity_tests::test_concurrent_writes_do_not_corrupt` still owed a fresh run — it asserts only that the document stays valid, which the measurement confirms. |
| **S2-8** | Two replicas against one data directory, with the filesystem type of the throwaway volume recorded — note that this checkpoint's volume is `tmpfs`, so this sub-task's result does **not** transfer to a real volume and the arm must be re-provisioned on a disk-backed directory before its result may be written up. |
| **S2-9 (partial)** | Lock release is measured for the **document** and **attachment** failure paths (pass entries 10–11). The revision path's failure-then-success pair has not been run, and no lock was observed *held* at any point (the probes measure release, not exclusion). |
| **S2-10** | Attachment publication in place (torn read), orphan handling, and revision immutability. Only the attachment lock half has run. |
| **S2-11** | The overwrite-contract table for every mutating operation, including the two imports whose conflict behaviour the design says is measured rather than assumed. |
| **S2-12 (partial)** | The error samples recorded so far are in §4's pending-triage and pass entries 7–11 (`storage_error` for corruption, the wrong-shape-JSON `200`, traversal `invalid_request` 400, conflict 409, not-found 404, the empty-body 404 fallback, unauthorized 401, and the length-overflow `500 storage_error` of `F-177-4`). The DoD item — the full `DomainError`-by-layer table — is not written, and `O-177-5` records that the storage failures collapse into one undifferentiated `storage_error`. |
| **S2-14 (partial)** | The auth surface is unreachable anonymously (§5, unnumbered note): `/auth`, `/auth//`, `/data/auth`, `/auth/projects`, `/projects/../auth` → **404**, `/auth/me` → **401**. What is owed is the **authenticated** arm, i.e. creating an auth store and confirming no project route can then reach it. |
| **S2-15** | The storage side of the configuration-file boundary, including the check of whether `#189`'s AEAD envelope has landed at the audited revision (which decides whether the *key* boundary is exercised or recorded as documented-pending). |

Also outstanding for the finished report: the README documentation-table row, the full local gate
(`actionlint`, `node scripts/check-matrix.mjs`, `cargo fmt --check`, `cargo clippy`, `cargo test`,
`cargo build --release`), and the pull request itself — which per the design is opened **only** when
the report is complete, assigned to `ECiurleo` and never merged by the auditor.
