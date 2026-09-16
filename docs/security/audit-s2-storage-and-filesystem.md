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
- **Findings so far:** **four**, all Low (`F-177-1`, `F-177-2`, `F-177-3`, `F-177-4`). §6 has
  confirmed the calibration for the two worked examples and for these four pairs; the **count** stays
  provisional because a sub-task still to run can add a finding.
- **Pass entries so far:** fourteen, in the [Pass entries](#5-pass-entries) section.
- **Executed:** S2-1, S2-2, S2-3 (partial), S2-4, S2-6 (partial), S2-7, S2-9
  (partial), S2-13, S2-14 (partial), S2-15 (partial — its `#189` question is answered and recorded in
  O-177-11; the file-boundary probes are owed with the stack). **Not executed:** S2-5, S2-8, S2-10,
  S2-11, S2-12 (partial).
- **Throwaway stack:** torn down, and the tear-down is recorded in [§7](#7-tear-down-step-7). A later
  checkpoint must re-provision before the outstanding sub-tasks can run.

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

## 3. Surface enumerated before probing (step 3 — S2-1, closed)

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

### 3.3 The closed set of mutating call sites

Enumerated with the sub-task's own commands:

```bash
grep -n "fs::rename\|fs::write\|fs::remove_file\|fs::remove_dir\|fs::create_dir\|File::create\|OpenOptions\|sync_all\|sync_data\|set_permissions\|fs::copy\|fs::hard_link" \
  src/storage/fs.rs src/storage/layout.rs src/auth/store.rs
grep -rn "fs::write\|fs::rename\|fs::remove_file\|fs::remove_dir\|fs::create_dir\|File::create\|OpenOptions" src/ --include=*.rs
grep -n "attachment_path(\|step_attachment_path(\|revision_dir(\|project_document_path(\|project_collection_dir(\|self.document(\|self.folder(" src/storage/fs.rs
```

The line numbers are the pinned revision's: `git diff c5e99431389854368ab3a8e07003622f34dfdd21 HEAD --stat -- src/ tests/ Cargo.toml`
is empty, so nothing in this report's source references can have drifted. Test code is excluded by
construction — the `#[cfg(test)] mod tests` boundaries are `src/storage/fs.rs:971`,
`src/storage/layout.rs:460`, `src/auth/store.rs:419`, `src/domain/service.rs:1679` — because the
sub-task enumerates the *service's* surface, and a test fixture that writes to its own `TempDir` is
not that surface.

| Site | Enclosing item | Mutation | Target comes from |
| --- | --- | --- | --- |
| `src/storage/fs.rs:30` | `FileRepository::new` (`:26`) | `create_dir_all(root/<name>)` per `ROOT_DIRS` | `ROOT_DIRS` literals (`layout.rs:49`), never request data |
| `src/storage/fs.rs:38` | `acquire_lock` (`:37`) | `OpenOptions` create `.tucano.lock` | `self.root.join(".tucano.lock")` |
| `src/storage/fs.rs:163`, `:164`, `:165`, `:174`, `:175`, `:178` | `write_json` (`:159`) | `create_dir_all(parent of destination)`, create `.tucano-<suffix>.tmp`, `sync_all`, `rename` onto `destination`, `remove_file` of the temp on failure | `destination` = always a `self.document(...)` / `project_document_path(...)` path; the temp name is the only locally built path, and it is `<validated directory>/.tucano-<random suffix>.tmp` |
| `src/storage/fs.rs:358`, `:362`, `:366` | `place_locked` (`:325`) | `create_dir_all(to.parent())`; `rename(from → to)` for `Placement::Move`; on a failed rename `copy_dir_all` + `remove_dir_all(from)`; `Placement::Copy` calls `copy_dir_all` | `from`/`to` = `self.folder(resource, Some(<parent>), id)` (`fs.rs:76`) |
| `src/storage/fs.rs:549`, `:551` | `delete_at` (`:546`) | `remove_dir_all(folder)` or, when the node is a document, `remove_file` | `self.folder(...)` / `self.document(...)` |
| `src/storage/fs.rs:601`, `:607`, `:609` | `save_attachment` (`:583`) | create `.tucano-<suffix>.tmp`, `sync_all`, `remove_file` of the temp on failure | `attachment_path(...)` (`layout.rs:323`), assigned at `fs.rs:600` |
| `src/storage/fs.rs:677` | `delete_attachment` (`:674`) | `remove_file` | `attachment_path(...)` |
| `src/storage/fs.rs:704`, `:706`, `:712`, `:714` | `save_revision` (`:618`) | `create_dir_all(revision dir)`, create temp, `sync_all`, `remove_file` of the temp on failure | `revision_dir(...)` (`layout.rs:304`), assigned at `fs.rs:646` |
| `src/storage/fs.rs:732` | `delete_step_attachment` (`:723`) | `remove_file` | `step_attachment_path(...)` (`layout.rs:353`), built inline at `:702`/`:732` |
| `src/storage/fs.rs:770`, `:778` | `probe_writable` (`:768`) | create `.tucano-<suffix>.tmp` in the **root**, `remove_file` it | `root.join(format!(".tucano-{}.tmp", unique_suffix()))` — the readiness probe, no request data |
| `src/storage/fs.rs:792` | `probe_lock` (`:791`) | `OpenOptions` create/read/write `.tucano.lock` in the root | `root.join(".tucano.lock")`; opened `truncate(false)`, so a probe never empties a held lock |
| `src/storage/fs.rs:856`, `:863` | `copy_dir_all` (`:855`) | `create_dir_all(to)`, `fs::copy` per entry | recursive over `fs::read_dir(from)` — names read back off the disk, not from a request; reached only from `place_locked` |
| `src/auth/store.rs:102` | `AuthStore::new` (`:100`) | `create_dir_all(grants_dir)` | `auth_dir()`/`grants_dir()` (`:106`/`:114`) |
| `src/auth/store.rs:136` | `AuthStore::acquire_lock` (`:135`) | `OpenOptions` create `.tucano.lock` | the auth directory |
| `src/auth/store.rs:361` | `remove_project_grants` (`:359`) | `remove_file` | `grant_path(project_id)` (`:123`) |
| `src/auth/store.rs:399`, `:401`, `:410`, `:411`, `:414` | `write_json_atomically` (`:395`) | `create_dir_all`, create temp, `sync_all`, `rename`, `remove_file` of the temp on failure | the caller's destination, by the same atomic-write pattern as `write_json` |
| `src/storage/layout.rs:455` | `set_private_permissions` (`:451`) | `set_permissions(0o666)` | the open write handle (`F-177-1`) |

**What the enumeration establishes.**

1. **Every mutating target is produced by the builders in `layout.rs`** — `project_document_path`
   (`:248`), `project_collection_dir` (`:215`), `revision_dir` (`:304`), `attachment_path` (`:323`),
   `step_attachment_path` (`:353`), and `fs.rs`'s own `document`/`folder` (`:49`/`:76`), which
   delegate to `project_document_path` (`fs.rs:70`), `project_dir` (`fs.rs:80`), `suite_dir`
   (`fs.rs:84`) and `case_dir` (`fs.rs:88`) in `layout.rs` (`:199`, `:261`, `:289`). Each of those
   validates the
   identifier and re-checks confinement (`validate_document_id` `:236`, `validate_component` `:367`,
   `ensure_within` `:391`, `resolve_existing_prefix` `:415`). This is the invariant the sub-task was
   written to test, and it holds at this revision.
2. **The only paths built outside `layout.rs` are the atomic-temp names** —
   `fs.rs:164`, `fs.rs:770`, `auth/store.rs:400` — each `<validated directory>/.tucano-<random
   suffix>.tmp`. They take their directory from a builder's output and their suffix from the random
   generator, so no request data reaches them; a temporary can therefore only ever appear beside the
   destination it is about to replace. That is also why a leftover `.tucano-*.tmp` is inert and
   unaddressable (pass entry 13's debris check, `O-177-1`).
3. **The lock file is created by two different items in the same module** — `acquire_lock` (`:37`,
   under `O_EXCL`-style `create(true)`) and `probe_lock` (`:791`, `truncate(false)`) — which is why a
   fresh data directory can hold a `.tucano.lock` before anything is written (`O-177-2` measures
   `0644` there).
4. **No path is built anywhere else in the service.** The repo-wide grep matched only three lines
   outside the three modules — `src/auth/config.rs:374`, `src/config.rs:378`, `:393` — all of them
   `std::fs::write` of a test fixture inside those files' `#[cfg(test)]` modules. Per the sub-task's
   expected result ("a path built anywhere else is a finding") there is **no finding** here.
5. **`refuse_legacy_layout` (`fs.rs:929`) does not mutate.** It is the one item in the
   persistence module whose name suggests a write; the enumeration shows it reads only, so a legacy
   tree that refuses the root is left untouched (the repository's own
   `a_refused_root_leaves_the_legacy_document_untouched` covers the same ground).

This closes S2-1: §3.1 names the enforcement points, §3.2 the permission call sites, and this table
every mutating call site, with the one deliberate exception above named rather than omitted.

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

**O-177-10 — Degenerate identifiers are accepted, and the design expected refusal; decided as an
observation, not a finding.** The design's S2-4 expectation is "Refusal (400 `invalid_…` or 409) for
**every** degenerate value", and it names `.tucano.lock`, `.tucano-1700000000000000000.tmp`, the
Windows device names, and whitespace-only among the values it expects refused. Measurement (the
pending-triage table) shows two of those classes **accepted**: a whitespace-only case id → **201**, and
a dotfile case id → **201**. The control that exists is narrower than the expectation, and the
repository states it exactly: `hostile_components_are_rejected` (`src/storage/layout.rs:770`) asserts
refusal only for `""`, `"."`, `".."`, a `/`-separated value, a `\`-separated value, and an absolute
path — names with *path* meaning — and asserts `TC-001.json` accepted. No baseline in the repository
claims a charset restriction. The accepted names are stored inside the actor's **own** project, are
returned by the same listing routes that would return any case (the case listing selects on
"directory holding `test-case.json`", not on a leading dot), and are deleted by the same routes that
created them; nothing is lost, disclosed, or defeated, so §5's impact axis has no loss to score and
the entry stays an observation. The reading not taken is named: under §5, *Moderate* × *Limited* would
be a **Low** finding and *Trivial* × *Limited* a **Medium** one; both presuppose an impact this
measurement does not show, and the second would also overstate exploitability, since the shipped
default requires a valid account and a project before a case id can be supplied at all. Two
consequences are recorded for a reviewer and are **not** scored here. (a) A case directory named
`.tucano.lock` or `.tucano-<suffix>.tmp` aliases the storage layer's *own* bookkeeping names; for the
lock file the alias is nominal rather than a collision, because `acquire_lock` opens the lock at the
data root (`src/storage/fs.rs::acquire_lock`, `self.root.join(".tucano.lock")`), one level above any
project, and a project child of the same name is a different entry. (b) `unique_suffix()`
(`src/storage/layout.rs:443`) is `SystemTime::now().as_nanos()` — **pure time, no randomness** — so
the atomic-temp name built at `src/storage/fs.rs:164` is in principle predictable, and an entry
already holding a temp's name in the
directory that receives the write makes `File::create` or `fs::rename` fail (a **500**), while a
*file* at that name is replaced. Exploiting it needs a nanosecond-exact guess of the service's own
clock reading, which is impractical rather than merely difficult, so it is recorded and not scored —
the same treatment §2.7 gives the leftover temp file, an inert bookkeeping name. **Recommended
regression tests (text only, per the design's "tests recommended, not written"):** extend
`hostile_components_are_rejected` with the accepted set so the control's boundary is asserted rather
than only measured; and plant a case whose id equals a temp name in the same directory, with a
matching suffix, then assert the sibling document write still succeeds.

**O-177-11 — The configuration-file loader has landed; the encrypted-secret envelope has not, so
S2-15's key boundary stays documented-pending.** The design leaves one question to the executor:
"**Whether `#189` has landed at the audited revision** changes S2-15's boundary from 'documented
pending' to 'must be exercised'." Queried 2026-09-16: `#188` ("[P3] Config: define and implement the
config file schema and loader") is **closed** (`closed_at 2026-09-14T15:17:32Z`); `#189` ("[P3]
Config: encrypted secrets at rest") is **open, `closed_at` null, no linked PR**; `#190` ("[P3] Config:
precedence and validation across file, environment, and defaults") is **open** too. So the answer is
**not landed**, and S2-15's *Configuration key* boundary is recorded as documented-pending rather
than exercised — which is the design's own pre-committed limit, and therefore not a finding. The
revision's documentation already says so in advance, in the *Decided* section of `threat-model.md`:
"**The file has no encryption yet**: #189's AEAD envelope and externally supplied key are still
pending, so a secret held in the file is in the clear and the *Configuration key* boundary above is
not yet exercised." Two pieces of evidence corroborate that the documented state is also the state of
the artifact. First, the source: a case-insensitive search of `src/` and `Cargo.toml` at the pinned
revision for `aead`, `xchacha`, `chacha20`, `envelope` and `encrypt` returns **only** the API's
error envelope (`src/api/error.rs`, `src/api/mod.rs:231`, `src/api/auth.rs`, `src/api/request_id.rs`)
and the doc-comment in `src/config.rs` — no AEAD type, no key-identifier field, and no crypto
dependency in the manifest. A secret in the configuration file is in the clear at this revision by
construction, not by omission of a probe. Second, the *loader* half **is** implemented and reachable,
which the same revision records: `src/config.rs` reads only the file named by the environment-only
`TUCANO_CONFIG_FILE`, and its error type "carries none of those by construction, so no `Display` impl
can leak them by accident" (`src/config.rs`, module docs). That makes S2-15's **file** half a real,
executable probe rather than a pending one: what it owes is the container arm the design specifies —
a read-only mount, a good file, an unknown key, a bad `version`, a malformed document, a missing file,
then `docker diff` plus an in-container `touch` to confirm invariant 9 (the service never writes the
file) and that no error text names a value. Those probes need the image, which is torn down (§7), so
they are owed rather than run. Two source-level baselines exist for them and were **re-run green** as
part of this checkpoint's baseline pass (see §5): `config::tests::no_error_text_carries_a_secret_value`
and `::an_unreadable_file_refuses_to_start_without_naming_the_path`. They are unit baselines over a
deliberately pure `resolve`, not container evidence, so this entry does **not** credit them as a pass
entry — it names them as what the owed probe will test against. **Not scored:** nothing here is
scored; the clear-text state is a documented, pre-committed limitation of the audited revision, and
the audit's job with respect to it is to record which state applied, which this entry does.
(Trust boundaries 6/7.)

**Pending triage — measured, and now decided.** The following results were produced by the S2-4
identifier probes. They are recorded so the measurement is not lost; every row is now either a scored
**finding**, a **pass entry**, or a recorded **observation**, and no row is left unscored:

| Probe | Result |
| --- | --- |
| case id `test_runs`, `milestones`, `configurations` (reserved project children), in a freshly created project | **409** `conflict` — refused, and refused in a project whose collection directory does not pre-exist |
| case id `.tucano.lock` | **201** — accepted as a case directory name; decided below (O-177-10) |
| case id `.tucano-1700000000000000000.tmp` | **201** — accepted (see O-177-1); decided below (O-177-10) |
| case id `CON`, `nul`, `aux` (Windows device names) | **201** — accepted; no refusal, so a Windows-hosted volume is the only place the name becomes special |
| case id `.`, `..`, `a/b`, `a\b` | **400** `invalid_request` — refused |
| case id `a%2Fb` | **201** — stored literally, no traversal |
| case id `""` | **400** `Required fields are missing` |
| case id `"   "` (whitespace only) | **201** — accepted, producing a whitespace-named directory; decided below (O-177-10) |
| case id 4096 bytes long | **500** `storage_error` `"Storage operation failed"` — **scored and promoted to `F-177-4`**; the boundary is `NAME_MAX` (`255` → 201, `256` → 500) |
| case id `cafe\u0301` (combining accent) | **201** — accepted as its own distinct identifier |
| duplicate case id `TC-LOGIN-1` | **409** `conflict` |
| project names `test_runs`, `milestones`, `configurations`, `smoke`, `audit probe` | **201** — project-level names are not subject to the reserved-child rule, as expected |
| suite created with body `{"suiteId":"test_runs","name":"t"}` (the invalid probe) | **201** with `{"id":"t.json"}` — the suite id is derived from `name`, so that probe did **not** test a reserved suite name |
| suite name `test_runs`, `milestones`, `configurations` in a **freshly created** project, re-run against `name` | **409** `conflict` — refused by validation, not by a pre-existing directory; the control `Smoke` → **201** and a repeat `Smoke` → **409** `conflict` |
| run and configuration names `test_runs` in the same fresh project | **201** each — recorded as O-177-9 (one level deeper, no collision) |

Three rows are closed by this checkpoint: the 4 KiB row became **`F-177-4`**, the invalid suite probe
was re-run against the correct field and confirms the reserved-child rule for suites, and the
whitespace-only and dotfile identifiers are decided in **O-177-10** — accepted by measurement,
expected refused by the design, and recorded as an observation rather than a finding because the
control the repository specifies and tests is the narrower one. **The pending table is therefore
empty of undecided rows**, and every value it lists is either a finding, a pass entry, or a recorded
observation.

## 5. Pass entries

Controls tested **and not broken** in this checkpoint:

1. **Traversal characters in an identifier are refused, not laundered.** `"."`, `".."`, `"a/b"`, and
   `"a\b"` as a case identifier all return **400** `invalid_request`, and the empty identifier returns
   **400** `Required fields are missing` (refused a layer earlier, by field validation); the request
   never reaches the filesystem layer in any of the five cases. (Trust boundary 3.)
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
14. **The identifier charset is deliberately open, and an open name stays inside the actor's own
    project.** The values the design lists as degenerate but the service accepts — a whitespace-only
    id, the dotfile names `.tucano.lock` and `.tucano-<suffix>.tmp`, the Windows device names, and a
    combining-accent value — are stored verbatim as the case directory, are returned by the case
    listing, and remain deletable through the same route that created them; no path leaves
    `Projects/<project>/…`, and no reserved collection is shadowed (the same names *are* refused at
    the project-child level, entry 3, and nest harmlessly one level deeper, O-177-9). The control is
    exactly `validate_component`'s refusals, not a charset restriction. Accepted by decision; the
    reading not taken is recorded in O-177-10. (Trust boundary 3.)

Outside the numbered entries, S2-14 found the same shape on the auth tree: `/auth`, `/auth//`,
`/data/auth`, `/auth/projects`, and `/projects/../auth` all return **404**, and `/auth/me` returns
**401** without a token — no route lists, reads, or writes the store under `TUCANO_DATA_DIR/auth/`.
That is recorded here rather than as an entry because it is a `partial` sub-task: the authenticated
arm (with an auth store actually created) has not been probed. (Boundary 6/7.)

**Repository baselines, re-run.** The baselines `audit-scope.md` names are not pass entries in their
own right, but S2-3, S2-4 and S2-7 each owe one, and S2-15's owed container probe has two, so they
were executed *after* the stack was torn down, on the host, against the audit worktree — whose
`src/`, `tests/`, `Cargo.toml` and `Cargo.lock` are byte-identical to the pinned revision
(`git diff c5e9943…dfdd21 HEAD` empty). Three commands, all green:

```text
cargo test --lib "storage::layout::tests::"   → 21 passed; 0 failed (346 filtered out; 0.00s)
cargo test --test security_tests              → 11 passed; 0 failed (0.00s)
cargo test --lib "config::"                   → 40 passed; 0 failed (327 filtered out; 0.00s)
```

That covers the four baselines S2-4 credits — `layout.rs::a_document_identifier_is_validated_before_any_path_is_built`,
`::hostile_components_are_rejected` (the baseline the O-177-10 decision rests on: it passes *because*
the set it refuses is the narrow path-meaning set), `::a_project_reserves_the_names_of_its_collections`,
`::case_folders_keep_their_identifier_verbatim` — the three symlink baselines S2-3 credits, and
`tests/security_tests.rs::data_integrity_tests::test_concurrent_writes_do_not_corrupt`, which passes
while asserting only that the stored document stays *valid*: validity holds and the durability of
every acknowledged write does not, which is exactly the distinction `F-177-3` records. The same
binary's `path_traversal_tests::*` (four tests, including
`test_rejects_traversal_through_a_case_identifier`) and `symlink_tests::test_rejects_symlink_escape`
also pass, independently corroborating pass entries 1–2 and 5. The third command is the `config::`
filter, which selects both the loader's own `config::tests::*` (16) and the auth layer's
`auth::config::tests::*` (24) — the baselines S2-15's owed probe will be measured against, including
`config::tests::no_error_text_carries_a_secret_value`,
`::an_unreadable_file_refuses_to_start_without_naming_the_path`, and
`auth::config::tests::no_startup_error_from_the_file_layer_carries_a_secret_value`. Passing them
credits nothing by itself: they exercise a deliberately pure `resolve` over an in-memory document and
say nothing about a read-only mount or `docker diff`, which is why S2-15 stays owed (O-177-11). This
is source-level evidence at the pinned revision, not evidence about the built image: the image was
audited by the API probes, the baselines by the test binaries compiled from the same revision.

Not yet credited in this checkpoint (and deliberately not listed as passes): atomicity under `SIGKILL`
(S2-5), two replicas on one data directory (S2-8), attachment and revision publication (S2-10), the
overwrite table (S2-11), the full error-leak table (S2-12), and the configuration-file boundary
(S2-15) — of which only the *file* half is still owed, and only because it needs a container: the
*key* half is settled by O-177-11 (`#189` is open, so the key boundary is documented-pending by the
design's own pre-commitment, not unprobed). The three symlink baselines correspond to the fixtures
measured in entries 5–6 above; the symlinked-*attachment* fixture S2-3 also names has not been
planted, so S2-3 stays partial. `F-177-3` names `test_concurrent_writes_do_not_corrupt` — now re-run
green — as the place a durability regression test belongs, because the baseline as written cannot
fail on an acknowledged-but-lost write.

## 6. Calibration confirmed

Confirmed at this checkpoint for the two worked examples and for the four findings written so far.
The count itself stays provisional, and the section says below what that costs and where it is
re-confirmed.

- **The Critical worked example** ([audit-scope.md](audit-scope.md) § 5): "With the shipped Compose
  configuration, `GET /openapi.json` is public by design, and suppose some route derived a filesystem
  path from a request field without confinement… *Trivial* × *Severe* → **Critical**." Re-confirmed,
  with the S2 surface's own part stated rather than borrowed from S3's: the band is unchanged, and on
  this surface the example's hypothesis **did not materialize**. Confinement holds where S2 measured
  it — pass entries 1–6 (traversal, symlink and hostile-component refusals on every path built from a
  request field, plus the reserved-collection rule) and O-177-10 — so all four S2 findings stay
  **inside the caller's own authorization scope**, and none of them could be scored on this band. The
  example's premise also holds at this revision, read from the served contract rather than restated:
  `openapi.json` declares `GET /openapi.json` with `security: []`, i.e. public by design, which matches
  the five public operations [authentication-decision.md](authentication-decision.md) names and S3's
  §6 records.
- **The Info worked example** ([audit-scope.md](audit-scope.md) § 5, left by it to "**#179** or #178 to
  confirm against the code"): `scripts/clear-data.mjs`. S4 confirmed it against the code and S3
  re-checked it at its own revision; re-read here, the facts are unchanged —
  `scripts/clear-data.mjs:25-32` is the fixed candidate list (`argv[2]`, `API_URL`, `TUCANO_API_URL`
  and the three localhost URLs, filtered) and `:46` falls back to the first candidate when none answers
  `/health`, and `Authorization` does not appear in the file at all. The calibration holds, nothing in
  S2 changes it, and it is **not** raised as a finding.
- **The four findings re-read against § 5.** Each states both axes and the matrix cell it reads off
  them, which the rubric requires before the pair becomes a number: F-177-1 *Difficult × Moderate* →
  **Low**, with the design's competing **Medium** reading ("another local principal on a default
  deployment") written down and the reason the lower one is taken; F-177-2 *Difficult × Moderate* →
  **Low**; F-177-3 *Difficult × Moderate* → **Low**; F-177-4 *Moderate × Limited* → **Low**. No
  finding is scored below its impact axis, and the two that could have escalated (F-177-1, F-177-2)
  state why escalation does not apply: neither is reachable in the shipped default without a position
  on the data volume. One consequence is recorded here rather than left for a reader to notice —
  **all four are Low, so §4's order is not a severity ranking.** It follows the order §3 enumerates the
  surface: the permission call sites (F-177-1), then the document path (F-177-2, F-177-3), then the
  identifier and error-class path (F-177-4). Comparing severity across S2's findings means comparing
  the pairs, not the sequence.
- **What this section does not yet confirm:** that four is the final count. S2-5, S2-8, S2-10, S2-11,
  S2-12 and S2-15 can each still add a finding, and a new finding changes the surface's distribution —
  which is why the header marks the count provisional. §6 is re-confirmed, not rewritten, in the
  closing checkpoint; the two examples and the four pairs above will not change unless a later
  sub-task contradicts one of them.

## 7. Tear-down (step 7)

Recorded 2026-09-16, at the end of the window that produced this checkpoint.

| Step | Command | Result |
| --- | --- | --- |
| Stop and remove the stack and its volume | `docker compose -p audit-177 down -v` | Container `audit-177-api-1` stopped and removed; network `audit-177_default` removed |
| Remove container-written files | `docker run --rm --entrypoint /bin/bash -v /tmp/audit-177/data:/w tucano-test-audit-177:c5e9943 -c 'cd /w && find . -mindepth 1 -maxdepth 1 -exec rm -rf {} +'` | Ran as `uid=10001(tucano)` — **the ownership workaround was needed**, exactly as for S3: the 142 entries the service wrote are owned on the host by `110000:100998` with `0755` directories, which the operator's own uid cannot unlink. The helper ran **before** the image was removed, so it could borrow the image's uid mapping |
| Remove the scratch directory | `rm -rf /tmp/audit-177` | Gone, including the `outside-canary.json` planted outside the tree's document area and every captured output |
| Remove the audit image | `docker rmi tucano-test-audit-177:c5e9943` | Untagged; `8db4b338b31c36d55fa168962c5d9522466a959c5bb86512652636c44e41a173` deleted |
| Confirm nothing is left | `docker ps -a --filter name=audit-177`; `docker images \| grep audit-177` | No container, no image |
| Confirm the operator's instance is untouched | `docker ps` | `tucano-test-api-1` still `Up 34 hours`, `tucano-test-gui-1` still `Up 34 hours`. **Note for the next reader:** `docker inspect` reports that container's `Config.Image` as `tucano-test-api:local` but its image id as `13f10c0e9208` (built 2026-09-09), while the tag `tucano-test-api:local` now resolves to `8f075af3c2e7` (built 2026-09-15) — the tag was rebuilt at some point without the running container being recreated. This audit did not build or re-tag that image and did not touch that container; the observation is recorded only so a later checkpoint does not read the mismatch as evidence of this window's work |

The throwaway volume was `tmpfs` on the host's `/tmp`, which is why S2-8's result cannot be taken from
this checkpoint's stack (see the *Not yet executed* row): the sub-task has to be re-provisioned on a
disk-backed directory. The audit's own scratch state is gone; the evidence that survives is this file
and the pushed commits.

## Not yet executed in this checkpoint

The following sub-tasks of [audit-design-176-178.md](audit-design-176-178.md) §"#177" §3 have not run
to completion. Each names what it is for, so a reader can see the shape of what is missing rather
than only its absence. Rows marked **partial** have measured results in §4/§5; what they still owe is
in the second column. S2-3, S2-6, S2-9 and S2-14 have run and keep a row only to name what their run
did **not** cover; S2-1, S2-4, S2-7 and S2-13 have now run in full, so their rows are gone; S2-15 has
run its stack-free half and keeps a row only for the container arm.

| Sub-task | What is missing |
| --- | --- |
| **S2-3 (partial)** | The six fixtures were planted and refused (pass entries 5–6, O-177-4), and the repository's own symlink baselines (`src/storage/layout.rs::a_symlink_that_escapes_the_root_is_rejected`, `::a_symlinked_collection_directory_that_escapes_the_root_is_rejected`, `::a_symlinked_project_folder_that_escapes_the_root_is_rejected`, `tests/security_tests.rs::symlink_tests::test_rejects_symlink_escape`) have now been **re-run green** (§5, "Repository baselines, re-run"). What is still missing is the **symlinked attachment** fixture: a symlinked *attachment file* inside a real case's attachments directory, pointing outside the root. |
| **S2-5** | Atomicity: ten `SIGKILL`s of the container process mid-write, then a JSON validation pass over every stored document and an inspection of leftover `.tucano-*.tmp` files. No power-loss durability is claimed either way; a missing parent-directory `fsync` is an observation by pre-commitment, never a finding. |
| **S2-6 (partial)** | Truncation, invalid UTF-8, and a 100 MiB replacement are measured (pass entries 7–9). The wrong-shape JSON case is measured **and is a finding** instead of a pass (`F-177-2`). No further variants are owed, and the calibration pass §6 owed `F-177-2` has now run; the row stays only until §6 is re-confirmed at the closing checkpoint. |
| **S2-8** | Two replicas against one data directory, with the filesystem type of the throwaway volume recorded — note that this checkpoint's volume is `tmpfs`, so this sub-task's result does **not** transfer to a real volume and the arm must be re-provisioned on a disk-backed directory before its result may be written up. |
| **S2-9 (partial)** | Lock release is measured for the **document** and **attachment** failure paths (pass entries 10–11). The revision path's failure-then-success pair has not been run, and no lock was observed *held* at any point (the probes measure release, not exclusion). |
| **S2-10** | Attachment publication in place (torn read), orphan handling, and revision immutability. Only the attachment lock half has run. |
| **S2-11** | The overwrite-contract table for every mutating operation, including the two imports whose conflict behaviour the design says is measured rather than assumed. |
| **S2-12 (partial)** | The error samples recorded so far are in §4's pending-triage and pass entries 7–11 (`storage_error` for corruption, the wrong-shape-JSON `200`, traversal `invalid_request` 400, conflict 409, not-found 404, the empty-body 404 fallback, unauthorized 401, and the length-overflow `500 storage_error` of `F-177-4`). The DoD item — the full `DomainError`-by-layer table — is not written, and `O-177-5` records that the storage failures collapse into one undifferentiated `storage_error`. |
| **S2-14 (partial)** | The auth surface is unreachable anonymously (§5, unnumbered note): `/auth`, `/auth//`, `/data/auth`, `/auth/projects`, `/projects/../auth` → **404**, `/auth/me` → **401**. What is owed is the **authenticated** arm, i.e. creating an auth store and confirming no project route can then reach it. |
| **S2-15 (partial)** | Its design-level question is answered and recorded: `#189`'s AEAD envelope has **not** landed at the audited revision (`#188` closed 2026-09-14, `#189` and `#190` open with no PR; the source search finds no crypto), so the *Configuration key* boundary stays **documented-pending** by the design's own pre-commitment rather than exercised, and the two `config::` baselines are re-run green (§5, O-177-11). What is still missing is the **container arm** of the file boundary: a well-formed file mounted read-only resolves; an unknown key, a bad `version`, a malformed document and a missing file each refuse startup with errors that name the setting and never the value; and the running service never writes the file (invariant 9), confirmed by `docker diff` and an in-container `touch`. Needs the image. |

Also outstanding for the finished report: the full local gate (`actionlint`, `node
scripts/check-matrix.mjs`, `cargo fmt --check`, `cargo clippy`, `cargo test`, `cargo build --release`)
and the pull request itself — which per the design is opened **only** when the report is complete,
assigned to `ECiurleo` and never merged by the auditor. The README documentation-table row the design's
PR step names is already added on this branch, pointing at this file next to its S3 and S4 siblings.
