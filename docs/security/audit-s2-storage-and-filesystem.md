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
- **Findings so far:** **seven**, with the pair behind each: Low are `F-177-1` (Difficult ×
  Moderate), `F-177-2` (Difficult × Moderate), `F-177-4` (Moderate × Limited) and `F-177-7`
  (Difficult × Moderate, the hardlink read-through S2-3's fixture 6 turned up); Medium are `F-177-3`
  (Moderate × **Moderate** — **re-graded in this checkpoint** from Low on S2-8's measurements, pass
  entry 24, the reason being recorded in the finding) and `F-177-5` (Trivial × Limited); High is
  `F-177-6` (Trivial × Moderate). The last two are what the configuration-file boundary's container
  arm produced. §6 has confirmed the calibration for the two worked examples and for all seven pairs;
  the **count** stays provisional because exactly one sub-task still to run — S2-12 — can add a
  finding, and §6's list of what it has re-read is updated below to seven.
- **Pass entries so far:** twenty-four, in the [Pass entries](#5-pass-entries) section.
- **Executed:** S2-1, S2-2, S2-3, S2-4, S2-6 (partial), S2-7, S2-8, S2-9, S2-13, S2-14
  (partial), **S2-15 (complete — both halves: the `#189` question is answered and recorded in O-177-11,
  and the file-boundary probes have run against the image, producing F-177-5, F-177-6, O-177-12 and pass
  entries 15–17)**. S2-3 is complete rather than partial as of this checkpoint: its owed symlinked
  *attachment* fixtures were planted and refused at the API (pass entries 18–19, O-177-13) and its
  fixture 6, the outside-file hardlink, was planted too and did not hold — that is F-177-7. S2-9 is
  complete as well: its third arm, the revision write, is pass entry 20. S2-10 is complete as well:
  its three arms — attachment publication, orphans, and revision immutability — are pass entry 21,
  with the torn-read recipe it was written against measured unreachable through the API. S2-5 is
  complete as well: ten timed `SIGKILL`s mid-write are pass entry 22, and it claims no power-loss
  durability. S2-11 is complete as well: its overwrite-contract table — every row measured against an
  existing identifier, against a missing one, with a post-failure leftover check — is pass entry 23,
  and it produced O-177-14. S2-8 is complete as well: it was re-provisioned on a disk-backed volume,
  and its three arms — two replicas on one data directory, the single-replica control, and the
  slow-writer race — are pass entry 24, which localises `F-177-3`'s mechanism without adding a finding
  of its own. It also produced O-177-15.
  **Not executed:** S2-12 (partial).
- **Throwaway stack:** re-provisioned for the S2-15 container arm with every build step `CACHED` from
  the pinned revision, and **retained** at `tucano-test-audit-177:c5e9943` for the next checkpoint; the
  earlier tear-down and the re-provisioning are both recorded in [§7](#7-tear-down-step-7). The S2-8
  pair is a separate stack on a disk-backed directory — see §7's *live at the end of this checkpoint*.
  A later checkpoint starts the outstanding sub-tasks with `docker compose -p audit-177 up -d`.

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

**Note on the image id.** The container arm of S2-15 needed the image after the tear-down, so it was
rebuilt with `docker compose build` and **every step reported `CACHED`** (about 1.4 s), from a tree
whose `Cargo.toml`, `Cargo.lock`, `src/`, `openapi.json`, `swagger.html` and `Dockerfile` are
byte-identical to the pinned revision (`git diff c5e9943…dfdd21 HEAD` empty, and the `Cargo.lock`
SHA-256 above is unchanged). The rebuilt artifact nevertheless reports a different id —
`sha256:562447fb6539ddd68567a4c67f016dc5f96b7dd338bb90a78217dc976679fd9a` for the manifest list, with
exported config `sha256:9358251216b11dc618b723673b0af73334ad6f425f2ba2a42cfecc4259ce5932` — because
buildx attaches attestations and a manifest list, so the *tag* is not a stable identifier for the
rootfs. All S2-15 measurements were taken against this rebuild and against the same revision as every
earlier measurement; the table's original row is kept rather than overwritten so the divergence is
visible. The image is **retained** for the next checkpoint.

Two facts in that table are load-bearing for every measurement below and are recorded rather than
hoped for:

- **The throwaway volume is on `tmpfs`, while a real deployment's `./data` is not.** The operator's
  long-lived deployment was inspected read-only for comparison and its `/data` reports
  `stat -f -c %T /data` → `ext2/ext3`. POSIX permission bits and symlink handling behave the same on
  both, so the *permission* results transfer; durability and `flock`-under-overlay results would not,
  and no such result is claimed in this checkpoint.
- **Authentication was off in the audited arm.** No `TUCANO_JWT_SECRET` was supplied, so the service
  ran in its default unauthenticated mode (see §2.1, note 6). Every request below was made
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
6. **Authentication off — recorded here, but *not* a deviation from the shipped Compose file.** The
   audited arm supplies no `TUCANO_JWT_SECRET` and sets no `TUCANO_AUTH_REQUIRED`; the shipped
   `docker-compose.yml` sets no auth variable either, and the service's own default is off
   (`src/auth/config.rs:130–133`: an absent `TUCANO_AUTH_REQUIRED` resolves to `false`; pinned
   `README.md:218` documents the `false` default and that "when off, every guard returns and the API
   is anonymous"; `docs/wiki/api-and-authentication.md` states "**Authentication is off by default.**").
   So this arm *is* the shipped configuration rather than a departure from it, and every finding that
   leans on the default's reachability rests on the shipped file rather than on an auditor's choice.
   The default configuration is intended to be audited this way (`audit-scope.md` §5's escalation rule
   turns on the *default* configuration), and every finding states the setting explicitly. An
   auth-enabled arm — `TUCANO_AUTH_REQUIRED=true` with a `TUCANO_JWT_SECRET` of at least the minimum
   length and one seeded account — is still required before the `Unauthenticated` and `Forbidden`
   error variants (S2-12) and the auth-store permission call site (`src/auth/store.rs:405`, F-177-1)
   may be described as measured; both are carried in §5.
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

- **Severity:** **Medium** — Moderate × Moderate, **re-graded in this checkpoint** from Low
  (Difficult × Moderate) on S2-8's measurements (pass entry 24). § 5 asks every change of score to
  state its reason, so that a later reader can disagree with the reason rather than with the number;
  both the reason and the readings that were **not** taken are here. The **impact** axis is unchanged
  and *Moderate*: the loss is silent, irreversible and leaves no history entry, but it stays inside
  the authorization scope of the writers entitled to that case. The **exploitability** axis moved
  from *Difficult* to *Moderate* for two reasons that converge. First, this is not the race the
  Difficult row means — a window the attacker cannot reliably reach — because it is won essentially
  every time: in the two-replica storm **23** of 50 acknowledged writes were discarded and in the
  single-replica control **25** of 50, and with an ordinary 1 MiB case document racing a small one
  **9 of 20** acknowledged writes left no trace, which is the Moderate row's own test, "repeatable
  without special conditions". Second, § 5's escalation clause applies on its own terms: the defect is
  remotely reachable in a **default** configuration and needs no unusual configuration, because the
  single-replica control shows it needs no second replica at all — the shipped stack is one
  container, and any client with write access to a case can destroy that case's edits with two
  concurrent requests. The earlier checkpoint declined that escalation on the ground that "no attacker
  gains anything — the loss is symmetric among the writers who are entitled to the resource"; that is
  a statement about the impact axis, where it is already priced as *Moderate*, while the escalation
  clause asks about reachability rather than about what the attacker gains. Readings considered and
  **not** taken: *Trivial × Moderate → **High*** — the Trivial row is defined by the input's reach
  ("no account and no special position, **single request** … no prerequisite state"), and this defect
  is not a single request: it needs two requests in flight against the same case at the same instant,
  and a case that already exists. The unauthenticated default is *why* § 5's escalation clause applies
  to this defect, not a reason to claim the Trivial cell, whose own wording ("single request … no
  prerequisite state") the defect does not meet; and *Difficult × Moderate → Low* as previously
  scored — the reading a reviewer
  may prefer if they hold a race to be Difficult however reliably it is won, which is why the measured
  loss rates are quoted above rather than summarised. Nothing here is scored below its impact axis.
- **In scope:** S2-7 and S2-8; trust boundary 4 (service → stored JSON); invariant 2 — the surviving
  document is always complete and always valid, but a write the API *acknowledged* can be absent from
  it.
- **Where:** the document write path, **localised in this checkpoint** — S2-8 (pass entry 24) is the
  measurement that made it attributable. `Service::update` (`src/domain/service.rs:212`) reads the
  stored document **with no lock held** (`:215–218`, the `read_at` at `:217`); `revise_case`
  (`:1061`) derives `current` from that unlocked read (`:1073`) and calls `save_revision`
  (`src/storage/fs.rs:618`), which takes the lock at `:625` and then, when a snapshot for `current`
  already exists, **returns `Ok(())` without writing anything** (`:635–637` — "A revision snapshot is
  immutable: an existing one is never rewritten"); `revise_case` then stores `version = current + 1`
  anyway (`service.rs:1083`), and only the final `write_marker` → `write_at` (`fs.rs:517`, lock at
  `:524`) is serialised. That is three separate critical sections, so the read-modify-write is not
  atomic, and the middle one makes the drop invisible: two writers that read the same version both
  claim the next one, the second one's snapshot is discarded silently, its document overwrites the
  first at the same version number, and both callers are answered `200`. The intended shape is
  visible in the same file — `save_attachment` (`fs.rs:583`) holds **one** lock from `:591` to `:614`
  across its whole file-plus-metadata read-modify-write, which is exactly what the document path does
  not do. The advisory lock itself is `acquire_lock` (`fs.rs:37`) over `root/.tucano.lock`, and its
  probe (`probe_lock`, `fs.rs:791`) reports it released between requests in every measurement of pass
  entry 24. The earlier checkpoint's version of this bullet said the mechanism was **inferred, not
  localised**; the inference was correct, and the chain above replaces it.
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

  S2-8 re-ran the reproduction where the filesystem type is the one a real deployment uses: on a
  **disk-backed** volume (`ext4`; `stat -f -c %T /data` → `ext2/ext3` inside the container), with the
  same directory mounted by **two** replicas (`127.0.0.1:3321` and `:3322`), then by **one** replica
  as a control, then with a 1 MiB `PUT` racing a small one. Commands, fixtures and the raw per-round
  status codes are in pass entry 24.
- **Observed.** Concurrent run: **all 32 requests returned `200`**, and the stored document reports
  `"version": 17` with **16** history entries and **16** files in `revisions/`. Sequential control:
  **all 20 requests returned `200`** with `"version": 21`, **20** history entries and **20**
  revisions. The concurrent run therefore acknowledged twice as many writes as it persisted. A
  tight reproduction — ten rounds of exactly two concurrent writers — is fully deterministic:
  `20` acknowledged writes, version `2 → 12` (delta `1` per round), **11** revisions on disk, and the
  surviving titles in `revisions/` show one writer per round, e.g. round 4 kept writer A and silently
  dropped writer B. No document was ever left malformed, no partial file appeared, and the failed
  writes of the over-long-identifier probe (F-177-4) left nothing behind.

  On the disk-backed volume the same loss appears, with the same shape, at **both** replica counts.
  Two replicas, 25 rounds × 2 concurrent `PUT`s against one data directory → **all 50 `200`**, the
  final document `{"title":"A-25","version":28}` identical on **both** replicas, **27** snapshots on
  disk: **23 acknowledged writes discarded**, in 23 of the 25 rounds. One replica, the same storm →
  **all 50 `200`**, `{"title":"Q-25","version":26}`, **25** snapshots: **25 discarded, one in every
  round**. Neither arm produced a single `409` or any other refusal, no `GET` ever saw a torn or
  interleaved document, and a walk of the whole tree found no `.tucano-*` temp file and no `*.tmp`
  left behind. The slow-writer arm — a 1 MiB `description` racing a small `PUT`, ten attempts —
  acknowledged **20** writes and kept **11** (`"version": 12`, `v1`…`v11`): **9 of 20 discarded**,
  and in attempts 2–9 the fast writer's value was never observable even in the `GET` issued after its
  own `200`. Attempt 1 is the exception that shows the sharper failure: the fast value (`FAST-1`,
  version 2) *was* visible to a client that read it back, and the next version on disk — `SLOW-1` at
  version 3 — reverted it, with both requests answered `200`.
- **Expected.** Either a write the service acknowledges is durable — 32 accepted writes produce 32
  versions — or a write that will not be applied is refused with a `409` conflict, as duplicate
  creation already is. An accepted `200` that leaves no trace is a false success signal, and a client
  cannot tell which of its two concurrent edits survived. The design's S2-8 expectation
  (`audit-design-176-178.md:1015–1017`) splits the same way against this measurement: *no torn
  document* held — no reader ever saw a half-updated document, at either replica count — while *the
  lock serialises the writers* did not: each critical section is serialised and the read-modify-write
  is not, which is the whole of the defect.
- **Impact.** Silent, non-recoverable loss of a collaborator's or of the same client's just-accepted
  edit. `changedFields` history loses the entry too, so the loss leaves no audit trail. Confined to
  the writers' own authorisation scope, so Moderate rather than Severe. S2-8 puts a rate on it: under
  exact concurrency the loss is the rule rather than an occasional window — 23 and 25 discarded writes
  out of 50, and 9 of 20 when one document is much larger — so a client whose request was accepted
  cannot rely on the value it reads back either, because attempt 1 of the slow-writer arm read its own
  value at version 2 and found it gone at version 3. The overwrite/locking
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
  concurrent clients and a case that already exists. In the shipped default — authentication off, no
  `TUCANO_AUTH_REQUIRED` set — that is any client that can reach the listener; under
  `TUCANO_AUTH_REQUIRED=true` it is a client holding write access on that case.

### F-177-4: An identifier longer than the filesystem's name limit is accepted and then fails as `500 storage_error`

- **Severity:** **Low** — Moderate × Limited. *Exploitability:* **Moderate**, on the rubric's "or one
  prerequisite step" clause — the request needs a project that already exists (the reproduction
  creates one first) and is then repeatable without special conditions. *Trivial* is declined because
  that row is defined by "no prerequisite state", which a project is not; it is **not** declined on the
  ground that an account is required, since the shipped default authenticates nothing (§ 2.1, note 6).
  *Impact:* **Limited** — the effect is confined to the caller's own request, and nothing is lost,
  disclosed, or defeated.
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
- **Impact.** Availability/error-class only, confined to the caller's own request: any client that can
  reach the listener — in the shipped default, with authentication off and the project prerequisite
  met — can produce `500`s at will, which pollutes monitoring and, more importantly, makes the
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

### F-177-5: A refused configuration file prints the error's `Debug` rendering, so the refusal names neither the setting nor the file

- **Severity:** **Medium** — Trivial × Limited. No attacker action is needed to reach the path: it is
  the refusal every misconfigured or partially-mounted deployment takes, which is precisely the case
  the boundary exists to handle, so the *Trivial* row of the matrix applies and that row reaches
  **Medium**. The impact stays Limited — the text reaches the refusing process's own console only, no
  value is disclosed, nothing is written. Neither adjustment applies: nothing here is remotely
  reachable in a default configuration (no escalation), and a de-escalation would require the defect
  to depend on an *unrecommended* configuration, whereas the audited revision's documented contract is
  that this refusal names the setting in **every** configuration.
- **In scope:** trust boundaries 6/7 (the configuration file as a service input) and 8 (error text is
  usable and discloses nothing); the S2-15 design probe's own words — "an unknown key, a bad
  `version`, a malformed document, and a missing file each **refuse startup** with an error that names
  the setting and never the value" — of which the refusal half holds and the *names the setting* half
  does not.
- **Where:** `src/main.rs:28` — `let file = config::load_from_env()?;` inside
  `async fn main() -> Result<(), Box<dyn std::error::Error>>`. Rust's `Termination` impl for `Result`
  prints the error's **`Debug`** rendering; the hand-written `Display` at `src/config.rs:183-196` —
  the impl that names `{CONFIG_FILE_ENV}` — is therefore never reached at process level, and the
  `#[derive(Debug)]` on `ConfigError` decides what an operator reads instead.
- **Affected revision:** `c5e99431389854368ab3a8e07003622f34dfdd21` (`tucano-test-audit-177:c5e9943`).
- **Reproduction.** With each fixture bind-mounted read-only at the path the environment variable
  names, one probe per input:

  ```text
  for f in config-unknown-key config-bad-version config-malformed; do
    docker run --rm -v /tmp/audit-177/$f.json:/etc/tucano/config.json:ro \
      -e TUCANO_CONFIG_FILE=/etc/tucano/config.json tucano-test-audit-177:c5e9943 \
      >out-$f.stdout 2>out-$f.stderr; echo "$f rc=$?"
  done
  docker run --rm -e TUCANO_CONFIG_FILE=/etc/tucano/absent.json tucano-test-audit-177:c5e9943 \
    >out-missing.stdout 2>out-missing.stderr; echo "missing rc=$?"
  ```

- **Observed.** All four inputs exit **1**, with stdout empty in every case (the missing-file run:
  `0` bytes stdout, `103` bytes stderr), and the text is the error type's `Debug` rendering:

  ```text
  Error: UnreadableFile { source: Os { code: 2, kind: NotFound, message: "No such file or directory" } }
  Error: Malformed { detail: "unknown field `jwt_secrett`, expected one of `version`, … at line 3 column 15" }
  Error: UnsupportedVersion { found: 2 }
  Error: Malformed { detail: "EOF while parsing an object at line 4 column 0" }
  ```

  `TUCANO_CONFIG_FILE` appears in **none** of the four, and the unreadable-file case names no path
  either, so an operator reading the failure cannot tell which setting was read or which file it
  resolved to. The rendering that *does* name the setting is unit-tested and green —
  `config::tests::an_unreadable_file_refuses_to_start_without_naming_the_path` asserts
  `rendered.contains(CONFIG_FILE_ENV)` — so the tested rendering is the one no operator ever sees: the
  test calls `Display` directly while the process reaches `Debug` implicitly through `Termination`.
- **Expected.** The refusal should reach the console as the path-free, value-free text `Display`
  already produces (for the unreadable case, text naming `TUCANO_CONFIG_FILE`), so the module's
  documented contract and the design probe's "names the setting" hold at the level where the operator
  actually reads them.
- **Impact.** Diagnostic only, yet it inverts the boundary's purpose: the refusal is the one moment at
  which the operator needs to know *which* setting was read, and the derived `Debug` also re-prints
  the whole `source` chain (`Os { code: 2, kind: NotFound, … }`) instead of the curated text — the same
  channel that carries a neighbouring boundary's raw path (O-177-12). A `Display`-only test cannot
  notice the difference, so the defect is invisible to the test suite that was written to prevent it.
- **Suggested fix.** Render with `Display` before exiting — e.g. catch the error at `main` and print
  `{e}` to stderr, or give `ConfigError` a hand-written `Debug` that forwards to `Display` — and add a
  **process-level** test that runs the binary with a missing file and asserts `TUCANO_CONFIG_FILE` in
  stderr, since the in-process unit test cannot reach the rendering the process chooses. The audit
  recommends the change and does not make it.
- **CWE:** no clean CWE applies, and it is recorded that way rather than forced onto a
  close-but-wrong entry: the refusal exists and does the right thing, only its rendering is wrong, so
  CWE-390 (no action taken) and CWE-778 (insufficient logging) both misdescribe it.
- **Duplicates / prerequisites:** not a duplicate of F-177-6 — that finding is a *value* appearing in
  one variant's text, and its cause (`src/config.rs:104` sweeping serde's message into an unconstrained
  `String`) survives any fix to this one; conversely, suppressing the leak by trimming `detail` would
  still leave `Debug`-only output here. The two share the delivery channel and differ in cause, fix and
  impact, which is why they stay two findings. Not a duplicate of F-177-1…4 (storage layer,
  boundaries 3 and 4).

### F-177-6: A configuration value whose JSON type contradicts its field is echoed verbatim in the startup error

- **Severity:** **High** — Trivial × Moderate. Trivial: the input is a one-line edit to a file the
  deployment already owns, and the refusal needs no privilege beyond reaching that file. Moderate: the
  text discloses the literal value the operator supplied, and the band's own definition names
  "credential disclosure" at this level. The competing *Limited* reading — which would score
  **Medium** — is written out below and deliberately not taken.
- **In scope:** trust boundaries 6/7 (the configuration file as a service input) and 8; the
  non-disclosure half of the boundary's contract as `src/config.rs`'s own test states it —
  `config::tests::no_error_text_carries_a_secret_value` — extended to the startup path where the
  failure actually lands. That test passes because it only ever exercises *correctly-typed* values.
- **Where:** `src/config.rs:104` — `detail: source.to_string()` in `ConfigFile::parse`: serde's message
  is copied verbatim into `ConfigError::Malformed { detail }`, an unconstrained `String`, and
  re-emitted. serde quotes the offending scalar when its JSON type contradicts the field's declared
  type (`invalid type: string "…", expected a boolean`); its other messages quote only the *key*
  (`unknown field \`jwt_secrett\``) or a position.
- **Affected revision:** `c5e99431389854368ab3a8e07003622f34dfdd21` (`tucano-test-audit-177:c5e9943`).
- **Reproduction.** A sentinel value typed into the wrong field with the wrong type, and three
  controls in which the same sentinel sits in a field whose type matches while the failure is provoked
  elsewhere:

  ```text
  printf '%s' '{"version":1,"auth_required":"SENTINEL-c0ffee-4815162342-abcdefghijklmnop"}' \
    >/tmp/audit-177/config-wrong-type.json
  docker run --rm -v /tmp/audit-177/config-wrong-type.json:/etc/tucano/config.json:ro \
    -e TUCANO_CONFIG_FILE=/etc/tucano/config.json tucano-test-audit-177:c5e9943 2>&1
  ```

  Controls: the same sentinel as `jwt_secret` (a string in a string field) with the failure provoked
  by an unknown key, by a bad `version`, and by a truncated document.
- **Observed.** Exit **1**, and:

  ```text
  Error: Malformed { detail: "invalid type: string \"SENTINEL-c0ffee-4815162342-abcdefghijklmnop\", expected a boolean at line 3 column 64" }
  ```

  `grep -c SENTINEL` → **1**. All three controls → **0**. The echo therefore needs the *type mismatch*,
  not merely a refusal: serde quotes the key for unknown fields and a position for parse errors. This
  is a property of the message the loader builds, so it is independent of F-177-5 — had `main` printed
  `Display`, the same `detail` would still have reached the console. The value chosen is the shape a
  `jwt_secret` would have if the operator put it in the wrong key with the wrong type, which is the
  misconfiguration class the "never the value" contract exists to cover.
- **Expected.** The refusal should carry serde's position and field name but never the scalar — e.g.
  replace serde's message for type errors with a two-field form (`expected: "boolean"`,
  `found: "string"`), or refuse to build `detail` from a message containing a quoted literal. Naming
  the offending key is desirable and does not require quoting the value.
- **Impact.** Disclosure of a supplied value into startup text — which is the standard first artifact of
  an incident, copied into a ticket or a bug report. The reading not taken is **Limited → Medium**: the
  leaked value is one the refused document could never have applied (the process exits before any
  setting is used), and the only reader is already someone with access to that container's console or
  log collector. It is stated here rather than omitted because it is defensible; **Moderate** is taken
  because "never the value" is unconditional in the design and in the module's own test file, and
  because the text is designed to be pasted into bug reports, where the audience is wider than the
  container it came from.
- **CWE:** CWE-209 (Generation of Error Message Containing Sensitive Information); CWE-532 (Insertion of
  Sensitive Information into Log File) as the delivery-side framing.
- **Duplicates / prerequisites:** not a duplicate of F-177-5 (different cause, fix and impact — see that
  entry's note), nor of F-177-1…4 (storage layer, boundaries 3 and 4). Related but distinct
  from O-177-12, a *path* echoed by a neighbouring setting's error that a `Display`-only fix would also
  silence, and unrelated to O-177-11's recorded state (`#189` not landed), because this leak is in the
  refusal text, not in the file.

### F-177-7: A hardlink is served as an attachment, because confinement is a path check

- **Severity:** Low
- **In scope:** S2 — storage and filesystem invariants; trust boundary 3 (*Resource ID or filename →
  filesystem*).
- **Where:** the class of controls, not one site: `src/storage/layout.rs:391` (`ensure_within`) and
  `:415` (`resolve_existing_prefix`) decide confinement from *paths*, and the attachment read
  (`GET /test_cases/{id}/attachments/{filename}`) then opens the path that check approved.
- **Affected revision:** `c5e99431389854368ab3a8e07003622f34dfdd21` (the image in §1).
- **Reproduction.** A hardlink from a file *outside* the data root onto in-tree names — with both ends
  on one filesystem, `ln /tmp/audit-177/outside-source.txt …data/projects/S23\ symlink\ fixture/C1/outside-hard.txt`
  and the same again as `…/outside-hard.json` in the project folder — is invisible to
  `resolve_existing_prefix` (there is no symlink to canonicalise) and to `validate_component` (the name
  is ordinary). All three routes then behave differently:
  - `GET /test_cases/C1/attachments/outside-hard.txt` → **200**, and the body is the outside file's
    content byte for byte: the outside file (`{"name":"outside-hard","projectId":"outside-hard"}`) is
    disclosed through the API.
  - `DELETE` of the same path → **200** `{"message":"File deleted successfully"}`, after which the
    outside file is **intact** — link count `3` → `2`, `sha256`
    `3886e9857855bcd71e95bed82e08f719c301b2d588ca8fcedfa9baaded756a23` unchanged. The unlink removes
    the in-tree name only, so there is no delete-through.
  - `POST` with that filename → **201**, storing the bytes as `1789537026504682289-outside-hard.txt`: the
    upload de-collides instead of opening the link, and the outside file is unchanged afterwards (same
    `sha256`, still `2` links). There is no write-through either.
  Of the three directions the read is the one that escapes: reading is the operation that follows the
  planted name, and no path check can see the second link.
- **Why this is a finding rather than a pass.** The design's S2-3 fixture 6 states its own expectation —
  "the hardlink cannot be turned into a read of an outside file **through the API**" — and that
  expectation is not met. The symlink half of the same sub-task passes (entries 5, 18, 19); the
  hardlink half does not, and the two are different controls, not one control with two fixtures: path
  confinement catches symlinks and is silent on hardlinks.
- **Severity — the two axes.** *Exploitability:* **Difficult.** The fixture requires write access to
  the storage tree, which the design's §3 excludes as a precondition, and on a host with
  `fs.protected_hardlinks` the source file must be one the planter already owns (or `CAP_FOWNER`).
  *Impact:* **Moderate** — the disclosure is of files the service uid can read anywhere on that
  filesystem, so a volume shared with another tenant or service turns into a read channel, though the
  data is served only to principals already authorized for the case. *Difficult × Moderate* → **Low**
  per [audit-scope.md](audit-scope.md) § 5. The excluded precondition is spent on the exploitability
  axis, where the design puts it, rather than as a second de-escalation — the rubric does not
  de-escalate below the impact axis, and de-escalating here would put the entry below its own Impact
  cell.
- **CWE:** CWE-59 (Improper Link Resolution Before File Access, "Link Following") — the control resolves
  links but does not consider non-link aliases of a file; recorded with the caveat that CWE-59's title
  names symlinks and this is a hardlink.
- **Fix direction (recommended, not applied — fixes are out of scope for this report).** A path check
  cannot close this; the stored-file reader has to look at the *file*. Refuse to read, serve, or unlink
  an attachment whose `nlink > 1` (one `symlink_metadata` call, already made one level up), and
  recommend the volume be a dedicated filesystem so "outside the root" and "outside the volume" stay
  the same statement. The regression test this report recommends belongs next to the symlink tests in
  `tests/security_tests.rs::symlink_tests` — a hardlink fixture beside `test_rejects_symlink_escape`,
  which no existing test covers: the repository's four symlink baselines all exercise the path check.
- **Duplicates / prerequisites:** not a duplicate of entries 5–6 or of pass entries 18–19, which measure
  the symlink control (and of entry 6, which measured an *in-tree* hardlink and a write, not an outside
  hardlink and a read). Related to F-177-1 in that both take the storage volume's trust properties as
  the deciding fact.

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
(S2-9) show that the write paths that can fail do fail cleanly — including the revision snapshot, the
arm added at this checkpoint (pass entry 20) — which is the part of writable-layer
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
measurement does not show, and the second would also overstate exploitability, since such a name can
only be supplied against a project that already exists — one prerequisite step, which is the Moderate
row rather than the Trivial one, whatever the authentication setting. Two
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
file) and that no error text names a value. Those probes need the image; at the time this entry was
first written the stack was torn down (§7), so they were owed rather than run, and two source-level
baselines were **re-run green** against them as part of this checkpoint's baseline pass (see §5):
`config::tests::no_error_text_carries_a_secret_value` and
`::an_unreadable_file_refuses_to_start_without_naming_the_path`. The probes have since run against the
rebuilt image (pass entries 15–17), and the outcome is the opposite of what those baseline names
suggest: both refusals hold, but the text the process prints is the `Debug` rendering (F-177-5) and one
variant of it echoes the value (F-177-6). The baselines are unit tests over a deliberately pure
`resolve`, not container evidence, so this entry still does **not** credit them as a pass entry — it
records what they were to be tested against and what the test actually showed. **Not scored:** nothing
here is scored; the clear-text state is a documented, pre-committed limitation of the audited revision,
and the audit's job with respect to it is to record which state applied, which this entry does.
(Trust boundaries 6/7.)

**O-177-12 — A `jwt_secret_file` naming a missing file reports the raw path in the startup text.** The
same window's container arm produced one result that belongs to the *key* half of the configuration
boundary rather than to the file half S2-15 is bounded to, so it is recorded here and not scored. With
`{"version":1,"auth_required":true,"jwt_secret_file":"/etc/tucano/absent-secret"}`, the run exits `1`
with

```text
Error: SecretFile { path: "/etc/tucano/absent-secret", source: Os { code: 2, kind: NotFound, message: "No such file or directory" } }
```

— a raw absolute path in the console text, which is the rule the ADR's secret-handling section states
and which `auth::ConfigError::SecretFile { path: String, … }` contradicts by construction. Two further
variants of that error type carry *values* by construction as well (`InvalidTtl { key, value }`,
`InvalidBool { value }`), which is the shape F-177-6 measures on the file boundary; a fix to F-177-5's
rendering would silence the path here without touching those. **Recorded, not scored**, for the reason
the design gives: S2-15 is bounded to the file named by `TUCANO_CONFIG_FILE`, and the secret-file
settings and their error text belong to the Configuration-key boundary's owner (#178) — the same
boundary split O-177-11 records for the `#189` question. It is kept because this run is the one that
produced it, and because #178 should test these variants with a sentinel rather than with a path.
(Boundaries 6/7, 8.)

**O-177-13 — The confinement refusal reaches the caller as a `500 storage_error`, and the status it does
not use still answers a question about the host.** The attachment probes of pass entries 18–19 produced
two refusals with different shapes for one and the same check. A name that is a symlink to an *existing*
path outside the root is refused as **500** `storage_error`; a symlink whose outside target is *absent*
is refused as **404** `not_found` — the same body as a name that has never existed. The `500` is the
layer's mapping of the `PermissionDenied` that `ensure_within` returns: it names no path and no setting,
so it keeps invariant 6, but it reports a condition no well-formed request can cause as a server fault.
The `404`/`500` split is, for a caller who can already plant a symlink inside the storage root, a
one-bit "does this outside path exist" oracle; that precondition is write access to the storage tree,
which is the capability boundary 3 exists to contain rather than to be probed from, so this is **recorded
and not scored** — and recorded chiefly so S2-12's `DomainError`-by-layer table does not rediscover it.
(Boundary 3; boundary 8.)

Two later observations are recorded in §5 instead of here, each next to the measurement that produced
it: `O-177-14`, the identifier the API reports against the one it accepts (pass entry 23), and
`O-177-15`, the readiness probe advancing `lastWriteUnix` (pass entry 24).

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
15. **Every bad configuration file stops the service before it serves anything.** With
    `TUCANO_CONFIG_FILE` naming a scratch file mounted read-only, all four inputs the design names —
    an unknown key, a bad `version`, a malformed document, and a missing file — exit **1** with stdout
    empty and no listener bound; nothing under `/data` is created or changed. The refusal half of the
    design's expectation therefore holds (invariant 8). What does **not** hold is the diagnostic half,
    which is `F-177-5`; this entry is deliberately limited to the refusal. (Invariant 8; boundaries
    6/7.)
16. **A well-formed configuration file resolves, and its settings are applied.** The same file with a
    valid document starts the service: the container reaches `Up`, `/proc/net/tcp` shows
    `00000000:0BB8 … 0A` (listening on `0.0.0.0:3000` as uid `10001`), and the settings took effect —
    the bootstrap pass created `/data/auth/users.json`, `/data/auth/projects`, `/data/projects` and
    `/data/.tucano.lock` under the mounted data directory. `docker logs` is **empty**: nothing is
    logged at startup, a fact S2-12's error-leak table will need for its log column. (Boundaries 6/7.)
17. **The running service never writes the file named by `TUCANO_CONFIG_FILE`.** Across a full
    start-and-serve run the host file is byte-identical: `sha256`
    `642bab0dd836c259cc4427429e18ff690fcb9d72106103e60705a7e14ba7040d`, mode `664`, and mtime/ctime
    `1789536326` all unchanged before and after. `docker diff` lists the mount as `C /etc`,
    `A /etc/tucano`, `A /etc/tucano/config.json` — an *addition* relative to the image with **no
    modification** entry — and in-container writes are refused twice over: as the service uid `10001` a
    `touch`/append fails `Permission denied` (rc 1), and isolated with `--user 0:0 --entrypoint sh` the
    same write fails `Read-only file system` (rc 1) with the host hash still unchanged. (Invariant 9;
    boundary 6/7 — the storage side of the configuration-file boundary.)
18. **A symlinked *attachment file* inside a real case folder is refused in every direction, and an
    upload of the same name is de-collided rather than followed.** With `escape.txt` planted in
    `/data/projects/S23 symlink fixture/C1/` as a symlink to `/tmp/outside-canary.txt` — the container's
    own tmpfs, so a write-through is visible inside the container and harmless on the host — `GET
    /test_cases/C1/attachments/escape.txt` returns **500** `storage_error` and `DELETE` of the same path
    also returns **500** `storage_error` (102-byte bodies, `{"code":"storage_error","message":"Storage
    operation failed"}`). `POST /test_cases/C1/attachments` carrying that filename returns **201** and
    stores the bytes as `1789536837251789433-escape.txt`: the collision is resolved by suffixing a new
    name rather than by opening the link. Nothing crossed the boundary — the outside file still reads
    `ORIGINAL`, `escape.txt` is still a symlink, and the case folder gained only the suffixed file. A
    *dangling* outside-root symlink gives the same answer as a name that was never there: `GET
    …/dangling.txt` → **404** `{"code":"not_found","message":"File not found"}`, identical to the control
    body apart from `requestId`, so the refusal does not report whether the target exists. (Boundary 3;
    invariant 1.)
19. **A symlinked *case folder* is unreachable rather than followed.** Replacing the case folder itself
    (`C1` → symlink to `/tmp`) makes the case unaddressable: `GET`, `DELETE` and `POST` on
    `/test_cases/C1/attachments/…` all return **404** `{"code":"not_found","message":"Test case not
    found"}`, even when the linked directory holds a valid `test-case.json` for that id and an
    `escapee.txt` reading `SENTINEL-ESCAPED` — the response body carries no trace of it
    (`grep -c SENTINEL` = 0). The container's `/tmp` was unchanged afterwards: no upload landed in it,
    and the outside file still read `ORIGINAL`. (Boundary 3; invariant 1.)
20. **A failed revision write releases the lock, and leaves no partial history behind.** S2-9's third
    arm, the revision path: with `revisions/` replaced by a root-owned regular file — the design's own
    recipe for making the destination unwritable — `PUT /test_cases/C1` with a changed field returned
    **500** `{"code":"storage_error","message":"Storage operation failed"}`, and the stored document was
    left untouched (`"title": "t"`, `"version": 1`), so the failed write is not half-applied. A
    `find /data -newermt '-1 minute'` ran immediately after the refusal and found no half-created
    folder, marker or entry — only the planted file and the case folder's own bumped mtime. Removing
    the plant and repeating the same `PUT` at once (`curl --max-time 10`) returned **200** in
    **0.0017 s**, which is the measurement that matters: the failing path had released the lock, so a
    later write does not wedge. That write then created `revisions/v1.json` holding the *pre-update*
    document (`"title": "t"`, `"version": 1`) and `GET /test_cases/C1/history` returned
    `[{"changedFields":["title"],"lastModified":"2026-09-16T05:32:26Z","version":1}]`, so the snapshot
    is of the version being replaced rather than of the incoming one. The snapshot lands
    `-rw-rw-rw-` (0666), the same file mode F-177-1 records for stored material — noted here so that
    finding's scope is not read as documents only, and not raised as a second finding. (Boundary 5;
    invariants 1 and 5.)
21. **A stored attachment name is never rewritten, a published revision is never rewritten, and a
    DELETE through the API leaves no orphan.** S2-10's three arms, run against the live stack.
    *Publication.* The design's torn-read recipe — re-uploading a larger file over the same stored
    name — is **unreachable through the API**: `POST /test_cases/C1/attachments` with
    `filename=1789536837251789433-escape.txt` returned **201** and de-collided to
    `1789537395849300108-1789536837251789433-escape.txt`, and `src/storage/fs.rs:511`'s
    `save_attachment` opens its destination with `.write(true).create_new(true)`, so an existing stored
    name is never opened for rewriting. What was measured instead is the *client-side consequence*:
    with a 3-byte file planted under a name the document declares as 8 bytes, `GET` returned **200**
    with `content-length: 3` and body `abc`. Nothing cross-checks the body against the document's
    recorded `size`, so a body shorter than what the document claims is served as a complete,
    successful response — the size in the document is a client-side convenience, not an enforced
    integrity check. (Boundary 4; invariants 1 and 5.)
    *Orphans.* `DELETE /test_cases/C1/attachments/<name>` returned **200**
    `{"message":"File deleted successfully"}` and removed **both** the file and its document entry
    (`version` 3→4) — no orphan — with the case's outside hardlink source still `2 links / 51 bytes`
    afterwards, so a delete neither follows nor damages a link out of the tree. A file removed
    *behind the API's back* leaves the document entry dangling (observable through the case document),
    and the read then returns a clean **404** `{"code":"not_found","message":"File not found"}`: no
    reconciliation pass notices the divergence. (Boundary 4.)
    *Revision immutability.* `revisions/v1.json`'s sha256
    (`dd51eaa2710b03d0eb1d03fb96bfd8dbd00908f1fb7a0cab1d36be67fbd4e1a3`) was unchanged across two
    further revisions (`v2.json` `e4e3b881314c41892e982e76d960c7e1ecea8c5b12bb242e71ca799b246480e0`),
    and the history endpoint grew `[1]` → `[1,2]` → `[1,2,3]` without any snapshot being rewritten —
    the immutability `src/storage/fs.rs:546` asserts with its early return on `revision.exists()`.
    (Boundary 5.)

Two S2-10 notes that are not entries. First, the history routes answer every write method with
**405 and an empty body**, in contrast to the `{"code":…,"message":…,"requestId":…}` shape every other
refusal in this report uses; the API's error contract expects a `code`, so this is a gap for S2-12's
table rather than a control. Second, a fixture caveat for any later comparison: the case file
`…/C1/1789536837251789433-escape.txt` was restored by hand after the short-body probe, from
`/etc/hostname`, and is **not** byte-identical to the original — sha256
`c5194672d129fc5ad717be5ee1cd7dea3bacc28e61f262b410a5dbb5c0868c74` against the original's
`c1d961124938394e3f5a6646de88e69535ad4d28799bcb987462a334fb574b21` — so no later hash comparison may
rest on that name.
22. **Ten `SIGKILL`s timed into a 1 MiB document write leave neither a half-written document nor a
    temp file behind.** S2-5's atomicity arm. Ten rounds: a `PUT /test_cases/C1` carrying a 1 MiB
    `description` was fired, and 30 ms later the service was killed with `docker compose -p audit-177
    kill` (`SIGKILL`) and restarted; **every** restart answered `/health` **200**. After the ten
    rounds: a walk of the whole data directory found **zero** files matching `.tucano-*` or `*.tmp`
    — the design's leftover-marker check; all **8** stored `*.json` documents parsed as JSON; and
    `GET /test_cases/C1` returned **200** in **0.003 s** with a complete 1,049,097-byte document. The
    write that won landed whole — `test-case.json` at `version` 4, with `revisions/v4.json` (486
    bytes) holding the *pre-update* snapshot, so the kill rounds produced no revision and no partial
    document. Publications by temp file plus rename therefore held across ten abrupt deaths.
    (Invariant 1; the DoD's atomicity item.)
    *What this arm does not claim,* by the design's own pre-commitment: nothing about power-loss
    durability — a missing parent-directory `fsync` stays an observation, never a finding — and the
    kills were timed by a 30 ms delay rather than by observing the write syscall, so the rounds are
    *consistent with* landing mid-write rather than proof of it. The per-round HTTP outcome was not
    captured (the `curl` output was discarded); the measured end state is what is recorded here.
    (Invariant 1; boundary 4.)
23. **Every mutating operation refuses to overwrite silently, and a move leaves exactly one home.**
    S2-11's overwrite contract, one `curl` per row against the throwaway stack, with the
    `find /data -newermt '-3 minute'` leftover check after the failing rows. The project identifier
    used throughout is `S23 symlink fixture.json` — see the note below on why the identifier the API
    reports is not the one it accepts.

    | Operation | Existing identifier | Missing identifier |
    | --- | --- | --- |
    | create case (`POST /projects/{id}/test_cases`) | **409** `conflict` "Resource already exists", stored document unchanged; a repeat of a fresh create is also **409** | **201** `{"id":"C-NEW","message":"Test case created"}` |
    | update case (`PUT /test_cases/{id}`) | **200** `{"message":"Resource updated"}` — replaces completely | **404** `not_found` |
    | delete case (`DELETE /projects/{id}/test_cases/{id}`) | **200** `{"message":"Test case deleted"}`, the case folder gone from the tree | **404** `not_found` |
    | duplicate case (`POST /test_cases/{id}/duplicate`, `{}`) | **201**, copy named `C-NEW-copy-1789537594067599685` (or the supplied `newId`); the source's sha256 was `67abdc62…` before **and** after | **404** "Test case not found" |
    | duplicate run (`POST /test_runs/{id}/duplicate`, `{}`) | **201**, copy named `R11-copy-1789537624927754518.json` | — |
    | import JSON (`POST /test_runs/{id}/import/json`) | **200** `{"imported":1,…}` the first time; the **same case re-imported with a different status** → **200** `{"imported":0,"skipped":1,"duplicates":1}`, and the stored run still reads `"status": "Passed"` | **404** "Test run not found" |
    | import JUnit (`…/import/junit`, `application/xml`) | **200** `{"imported":1,…}`, the result filed under `C1.a` (the `classname`/`name` pair) | — |
    | compose copy (`POST /test_suites/{id}/test_cases`) | **201** `{"id":"C-COPY2","message":"Test case copied"}`; the source case directory stays in the project | **404** "Test case not found" |
    | compose move (same route, `"mode":"move"`) | **201** `{"id":"C-NEW","message":"Test case moved"}`; the case directory left the project and is now the single home `<project>/S11s/C-NEW/`, the suite holding `<project>/S11s/suite.json` | — |
    | compose move onto a case already in that suite | **409** `conflict` "This identifier is used by 2 parents (S23 symlink fixture.json, S23 symlink fixture.json/S11s.json); address the intended one through …" — refused, not silently re-homed | — |
    | bad `mode` | **400** `invalid_request` "Field `mode` must be `copy` or `move`" | — |
    | suite removal (`DELETE /test_suites/{id}/test_cases/{case_id}`) | **200** `{"message":"Test case removed from suite"}`; a repeat is **404** `not_found` | — |

    The leftover check found nothing half-created: every path newer than the probe window belonged to
    an entity the probes had made (`C-COPY2`, the copy, the suite, `test_runs/R11.json` and its copy,
    and `C1`'s own revision), so no failed row left a folder, marker, or entry behind. (Boundary 4;
    invariant 1.)

Two notes from S2-11 that are not entries. First, `O-177-14`, a measured divergence between the
identifier the API **reports** and the one it **accepts**: `POST /projects` answers with a `projectId`
(`S23sym`), while `GET /projects` lists `S23 symlink fixture.json`, and only the second form works in a
route — `GET /projects/S23sym/test_cases` and `GET /projects/S23%20symlink%20fixture/test_cases` are
both **400** `invalid_id`, `GET /projects/S23sym.json/test_cases` is **404** "Project not found", and
`GET /projects/S23%20symlink%20fixture.json/test_cases` is **200**. Recorded as an observation rather
than a finding: it is an identifier-contract defect, not a confinement one — every route still resolves
to a path inside the project directory — but the rows above are unreachable unless the caller knows to
append `.json` to the *name*, and the initial probe batch of this sub-task was refused with
`invalid_id` for exactly that reason. Second, the project's older cases are not covered by the rows
above: `C1` was the fixture case the earlier sub-tasks planted, so the table's "existing identifier"
column is exercised by cases this checkpoint created, not by import of a pre-existing tree.

24. **Two replicas on one disk-backed data directory acknowledge fifty concurrent writes and discard
    twenty-three of them; one replica discards one write in every round.** S2-8, re-provisioned onto a
    real volume because the previous checkpoint's row could not be answered from a `tmpfs` fixture —
    the design requires the arm's filesystem to be measured (`audit-design-176-178.md:1018–1020`, "an
    overlay/tmpfs result does not transfer to a real volume"). `findmnt` on the host gives the bind
    source `/var/tmp/audit-177-disk` → **`ext4 /dev/nvme0n1p2`**, and inside *both* containers
    `stat -f -c %T /data` answers **`ext2/ext3`** — a real block-device filesystem, so the caveat is
    satisfied. Two replicas (`a` on 3321, `c` on 3322) mounted that directory and ran 25 rounds of two
    concurrent `PUT /test_cases/C1`, the bodies differing only in `title` (`A-<n>` from one replica,
    `C-<n>` from the other). **All 50 requests returned `200`**, both replicas then served the
    byte-identical document
    `{"expectedResult":"e","lastModified":"2026-09-16T10:14:44Z","testCaseId":"C1","title":"A-25","version":28}`,
    and `revisions/` held **27** snapshots (`v1`…`v27`): 50 acknowledged writes, 27 applied versions,
    **23 gone** — in 23 of the 25 rounds; two rounds applied both — and neither replica ever answered
    `409` or refused anything. A walk of the whole tree found no `.tucano-*` temp file and no `*.tmp`,
    the only dot-entry in the data root is `data/.tucano.lock`, and all **67** stored `*.json`
    documents parse. The loss is silent in every sense: nothing refused, nothing malformed, nothing
    left behind.
    *One replica is enough.* The same storm against a **single** container (case `C2`, 50 requests)
    also returned **all 50 `200`** and left `{"title":"Q-25","version":26}` over **25** snapshots:
    **25 discarded, exactly one per round**. The control is what makes the first arm attributable — it
    shows the loss happens inside one process, so it is `F-177-3`'s unlocked read-modify-write and not
    a lock-scope effect, and it is reachable in the shipped single-container deployment. The `Where`
    bullet of that finding carries the code-level chain this arm localises.
    *A large document widens it.* Ten rounds of one slow 1 MiB `PUT` (case `C3`) racing one small `PUT`
    acknowledged **20** writes and stored **11** (`"version": 12`, `v1`…`v11`): **9 discarded**. In
    attempts 2–9 the fast value is not observable even in the `GET` issued immediately after its own
    `200`; in attempt 1 it was observable and then reverted — mid-flight `FAST-1 FASTE-1 v2 d0`,
    final `SLOW-1 SLOWE-1 v3 d1048576` — so a client can read a value back and later find it gone.
    Every `c3-slow-*.code` recorded `200`.
    *At rest both replicas are healthy.* `/diagnostics` →
    `{"exists":true,"lastWriteUnix":1789553864,"lockHeld":false,"lockable":true,"ready":true,"storage":"filesystem","writable":true}`
    and `/ready` → `{"status":"ready","storage":"filesystem"}`; the lock is not held between requests,
    which is the mechanism rather than a fault.
    *What this arm does not claim.* Nothing about the lock's scope across hosts: both replicas share
    one `flock` on one machine, which the design records as an Info observation and this entry does
    **not** score — the single-replica control is what removes it as a cause. The divergence window's
    *size* is not measured, only its existence and its rate. The arm adds **no** finding of its own.
    Read against the design's expectation (`audit-design-176-178.md:1015–1017`) the split is exact:
    "no torn document" **held** — every document read back was complete and none interleaved — while
    "the lock serialises the writers" **did not**: each critical section is serialised and the
    read-modify-write that spans them is not, which is the whole of `F-177-3`. (Boundary 5;
    invariant 1; the DoD's concurrency item.)

**O-177-15 — `lastWriteUnix` is advanced by the readiness probe itself, so the field does not separate
"writers have stalled" from "the operator polled `/ready`".** `probe_readiness`
(`src/storage/fs.rs:745–761`) reads `newest_mtime` (`:751`, implemented at `:813` over the data root
and its immediate entries) *before* `probe_writable` (`:768`) creates and removes its own scratch file
`root/.tucano-<suffix>.tmp` in that same root. The probe therefore leaves its own file out of the value
it returns — but creating and removing it still bumps the **data root's** mtime, which the *next*
probe reads as the newest write. Measured on the S2-8 stack: with the data root at
`2026-09-16 07:17:44.775442391 -0300`, `/diagnostics` reported `lastWriteUnix 1789553864`; one
`GET /ready` later the root mtime was `07:18:48.357905454` and the next `/diagnostics` reported
`lastWriteUnix 1789553928` — the only writer in that interval was the probe. Recorded as an
observation, not a finding: the scratch file is removed, nothing is over-permissive, and the endpoint
is documented as a readiness report, not as an audit log. What it costs is diagnosability: an operator
who monitors `/ready` keeps the field moving by monitoring it, and since `write_json` uses the same
`.tucano-<suffix>.tmp` convention, a fresh `lastWriteUnix` is not evidence that a *client* write
landed. (Boundary 4.)

Outside the numbered entries, S2-14 found the same shape on the auth tree: `/auth`, `/auth//`,
`/data/auth`, `/auth/projects`, and `/projects/../auth` all return **404**, and `/auth/me` returns
**401** without a token — no route lists, reads, or writes the store under `TUCANO_DATA_DIR/auth/`.
That is recorded here rather than as an entry because it is a `partial` sub-task: the authenticated
arm (with an auth store actually created) has not been probed. (Boundary 6/7.)

**Repository baselines, re-run.** The baselines `audit-scope.md` names are not pass entries in their
own right, but S2-3, S2-4 and S2-7 each owe one, and S2-15's container probe named two, so they
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
`auth::config::tests::*` (24) — the baselines S2-15's probe was to be measured against, including
`config::tests::no_error_text_carries_a_secret_value`,
`::an_unreadable_file_refuses_to_start_without_naming_the_path`, and
`auth::config::tests::no_startup_error_from_the_file_layer_carries_a_secret_value`. Passing them
credited nothing by itself: they exercise a deliberately pure `resolve` over an in-memory document and
said nothing about a read-only mount or `docker diff`, which is why S2-15 was owed (O-177-11). The
probe has since run against the image (pass entries 15–17), and the two `config::` baselines are the
clearest illustration in this report of why a green baseline can be narrower than its name suggests:
`an_unreadable_file_refuses_to_start_without_naming_the_path` passes while the process prints the
`Debug` rendering in which no setting is named (`F-177-5`), and
`no_error_text_carries_a_secret_value` passes because it only ever exercises correctly-typed values,
which is the one case that does not echo the value (`F-177-6`). This is source-level evidence at the
pinned revision, not evidence about the built image: the image was
audited by the API probes, the baselines by the test binaries compiled from the same revision.

Not yet credited in this checkpoint (and deliberately not listed as passes): the full error-leak table
(S2-12). Five sub-tasks have left this list since the
previous checkpoint:
atomicity under `SIGKILL` (S2-5), whose ten timed kills and clean-aftermath walk are pass entry 22,
the overwrite contract (S2-11), whose per-operation table is pass entry 23,
attachment and revision publication (S2-10), whose three arms are pass entry 21, and the
configuration-file boundary (S2-15), both of whose halves are settled — the *key* half by O-177-11 (`#189`
is open, so the key boundary is documented-pending by the design's own pre-commitment, not unprobed),
the *file* half by pass entries 15–17 after the container arm ran. The fifth is the two-replica arm
(S2-8), whose three arms are pass entry 24: it was re-provisioned on a real volume because the row it
left behind said a `tmpfs` result would not transfer, and it is the measurement `F-177-3`'s re-grade
rests on. The three symlink baselines
correspond to the fixtures measured in entries 5–6 above; the symlinked-*attachment* fixture S2-3 also
names has since been planted and refused at the API — the attachment file, the dangling variant of it
and the case folder itself (pass entries 18–19, O-177-13) — so S2-3 is no longer partial; its fixture 6
(the outside-file hardlink) has since been planted as well, and it did **not** hold, which is why it
appears in §4 as F-177-7 rather than on this list. `F-177-3` names
`test_concurrent_writes_do_not_corrupt` — now re-run green — as the place a durability regression test
belongs, because the baseline as written cannot fail on an acknowledged-but-lost write; pass entry 24
measures that loss on a disk-backed volume and `F-177-3`'s *Where* bullet localises it in the code.

## 6. Calibration confirmed

Confirmed at this checkpoint for the two worked examples and for the seven findings written so far. The
count itself stays provisional, and the section says below what that costs and where it is
re-confirmed.

- **The Critical worked example** ([audit-scope.md](audit-scope.md) § 5): "With the shipped Compose
  configuration, `GET /openapi.json` is public by design, and suppose some route derived a filesystem
  path from a request field without confinement… *Trivial* × *Severe* → **Critical**." Re-confirmed,
  with the S2 surface's own part stated rather than borrowed from S3's: the band is unchanged, and on
  this surface the example's hypothesis **did not materialize**. Confinement holds where S2 measured
  it — pass entries 1–6 (traversal, symlink and hostile-component refusals on every path built from a
  request field, plus the reserved-collection rule) and O-177-10 — so the five storage findings stay
  **inside the caller's own authorization scope** (F-177-7 included: reaching the hardlink's name
  already requires a case the caller can read), and none of them could be scored on this band. The
  two findings the configuration-file boundary added are not candidates for the example either, and for
  a stronger reason than a failed hypothesis: they are decided **before the listener binds**, so no
  request field exists yet and path confinement is not the control in question — F-177-6's disclosure
  reaches the refusing process's own console, which is why it is scored on boundary 8 rather than on
  boundary 3. The example's premise also holds at this revision, read from the served contract rather
  than restated: `openapi.json` declares `GET /openapi.json` with `security: []`, i.e. public by
  design, which matches the five public operations
  [authentication-decision.md](authentication-decision.md) names and S3's §6 records.
- **The Info worked example** ([audit-scope.md](audit-scope.md) § 5, left by it to "**#179** or #178 to
  confirm against the code"): `scripts/clear-data.mjs`. S4 confirmed it against the code and S3
  re-checked it at its own revision; re-read here, the facts are unchanged —
  `scripts/clear-data.mjs:25-32` is the fixed candidate list (`argv[2]`, `API_URL`, `TUCANO_API_URL`
  and the three localhost URLs, filtered) and `:46` falls back to the first candidate when none answers
  `/health`, and `Authorization` does not appear in the file at all. The calibration holds, nothing in
  S2 changes it, and it is **not** raised as a finding.
- **The seven findings re-read against § 5.** Each states both axes and the matrix cell it reads off
  them, which the rubric requires before the pair becomes a number: F-177-1 *Difficult × Moderate* →
  **Low**, with the design's competing **Medium** reading ("another local principal on a default
  deployment") written down and the reason the lower one is taken; F-177-2 *Difficult × Moderate* →
  **Low**; F-177-3 *Moderate × Moderate* → **Medium**, **re-graded in this checkpoint** from
  *Difficult × Moderate* → Low on S2-8's measurements (pass entry 24), because the loss is won in
  essentially every round rather than in an occasional race and because the escalation clause reaches a
  defect the shipped single-container stack hits, with both the reason and the two readings *not* taken
  (*Trivial × Moderate* → High; *Difficult × Moderate* → Low) written into the finding; F-177-4
  *Moderate × Limited* → **Low**; F-177-5
  *Trivial × Limited* → **Medium**, the Trivial row of the matrix, with the de-escalation question
  answered in the entry (the defect does not depend on an unrecommended configuration — the revision's
  contract is that the refusal names the setting in every configuration); F-177-6 *Trivial × Moderate*
  → **High**, with the competing *Limited → Medium* reading written down and the reason the higher one
  is taken; F-177-7 *Difficult × Moderate* → **Low**, the hardlink arm of S2-3's own fixture 6, with
  the excluded precondition spent on the exploitability axis. No finding is scored below its impact
  axis. Of the three the escalation clause could have reached, two do not escalate — F-177-1 and
  F-177-2 state why: neither is reachable in the shipped default
  without a position on the data volume — and the third, F-177-3, does, for the reason recorded above
  and in the finding. One consequence is recorded here rather than left for a reader
  to notice — **four of the seven are Low, two are Medium (F-177-3, re-graded, and F-177-5) and one is
  High, so §4's order is neither a severity ranking nor an order by band.** It follows the
  order §3 enumerates the surface: the permission call sites (F-177-1), then the document path
  (F-177-2, F-177-3), then the identifier and error-class path (F-177-4), and finally the
  configuration-file boundary, whose two findings were written last because they were measured last.
  Comparing severity across S2's findings means comparing the pairs, not the sequence.
- **What this section does not yet confirm:** that seven is the final count. S2-12 is the one
  sub-task left that can still add a finding, and a new finding changes the surface's distribution —
  which is why the header marks the count provisional. §6 is re-confirmed, not rewritten, in the closing
  checkpoint; the two examples and the seven pairs above will not change unless a later sub-task
  contradicts one of them.

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

The throwaway volume of the first stack was `tmpfs` on the host's `/tmp`, which is why S2-8's result
could not be taken from it: the sub-task was re-provisioned on a disk-backed directory, and that is
pass entry 24 (see also *live at the end of this checkpoint* below). The audit's own scratch state is
gone; the evidence that survives is this file and the pushed commits.

**Re-provisioned, later in the same window, for S2-15's container arm.** The sub-task's file half needs
the image, so the stack was rebuilt from the same revision with `docker compose build` — every step
`CACHED`, about 1.4 s (the build cache survived the `docker rmi` above), producing the manifest-list id
recorded in §1's *Note on the image id* — and `/tmp/audit-177/` was recreated with `compose.yml`, the
six configuration fixtures, the captured `out-*.txt` files and the split stdout/stderr pair. The image
was **deliberately retained**, and only the one named probe container (`audit-s2-15-good`) was removed,
so the remaining sub-tasks start with `docker compose -p audit-177 up -d` instead of a rebuild. Nothing
else was left running: `docker ps` reports only the operator's `tucano-test-api-1`, `tucano-test-gui-1`
and `open-webui`. This is a change of state from the tear-down table above and is recorded here rather
than by editing that table, which keeps the record of what the tear-down did.

**Live at the end of this checkpoint.** Three audit containers are up, all from the pinned revision's
image: the S2-8 pair `audit-177-disk-a-1` (127.0.0.1:3321) and `audit-177-disk-c-1` (127.0.0.1:3322),
each bind-mounting `/var/tmp/audit-177-disk/data`, plus the earlier `audit-177-api-1` (3320) on the
`tmpfs` fixture at `/tmp/audit-177/data`. The S2-8 fixtures sit on `/var/tmp`, which is `ext4` on this
host and survives a reboot, so the pair's state — `data/.tucano.lock`, `projects/S28/` with `C1` (27
revisions, `version` 28), `C2` (25, 26) and `C3` (11, 12), and the `s28-storm.log`, `s28-single.log`,
`s28-stale.log` and `c3-slow-*.{json,code}` transcripts — is the evidence pass entry 24 is written
from and can be re-read by a later checkpoint. The `/tmp/audit-177/` fixtures are on `tmpfs` and are
not: they survive only until the next reboot.

## Not yet executed in this checkpoint

The following sub-tasks of [audit-design-176-178.md](audit-design-176-178.md) §"#177" §3 have not run
to completion. Each names what it is for, so a reader can see the shape of what is missing rather
than only its absence. Rows marked **partial** have measured results in §4/§5; what they still owe is
in the second column. S2-6 and S2-14 have run and keep a row only to name what their run
did **not** cover; S2-1, S2-3, S2-4, S2-5, S2-7, S2-8, S2-9, S2-10 and S2-13 have now run in full, so their rows are gone
(S2-5's ten timed `SIGKILL`s are pass entry 22 and it claims no power-loss durability;
S2-9's third arm, the revision write, is pass entry 20; what S2-9 still cannot
show is a lock observed *held* — its probes measure release, not exclusion; S2-10's three arms are
pass entry 21, and what S2-10 still cannot show is a torn body: the recipe that was to produce one
turned out unreachable, so the *consequence* of a short body was measured instead, not its
production; S2-8's three arms are pass entry 24, run on a disk-backed volume as its old row demanded,
and it claims nothing about cross-host lock scope), and
S2-15 has now run both halves — the `#189` question in O-177-11 and the file-boundary probes in pass
entries 15–17 — so its row is gone as well (its two findings and one observation are in §4). S2-3's row
is gone for the same reason: the attachment fixtures it owed are pass entries 18–19, and its fixture 6
ran too — as F-177-7 in §4, because the hardlink arm did not hold.

| Sub-task | What is missing |
| --- | --- |
| **S2-6 (partial)** | Truncation, invalid UTF-8, and a 100 MiB replacement are measured (pass entries 7–9). The wrong-shape JSON case is measured **and is a finding** instead of a pass (`F-177-2`). No further variants are owed, and the calibration pass §6 owed `F-177-2` has now run; the row stays only until §6 is re-confirmed at the closing checkpoint. |
| **S2-12 (partial)** | The error samples recorded so far are in §4's pending-triage and pass entries 7–11 (`storage_error` for corruption, the wrong-shape-JSON `200`, traversal `invalid_request` 400, conflict 409, not-found 404, the empty-body 404 fallback, unauthorized 401, and the length-overflow `500 storage_error` of `F-177-4`, plus the history routes' **405 with an empty body** measured for S2-10). The DoD item — the full `DomainError`-by-layer table — is not written, and `O-177-5` records that the storage failures collapse into one undifferentiated `storage_error`. One measured fact already belongs in that table's log column: the service logs **nothing** at startup, cleanly or otherwise (pass entry 16), so a failure that only appears in the console is the configuration refusal and nothing else. |
| **S2-14 (partial)** | The auth surface is unreachable anonymously (§5, unnumbered note): `/auth`, `/auth//`, `/data/auth`, `/auth/projects`, `/projects/../auth` → **404**, `/auth/me` → **401**. What is owed is the **authenticated** arm, i.e. creating an auth store and confirming no project route can then reach it. |

Also outstanding for the finished report: the full local gate (`actionlint`, `node
scripts/check-matrix.mjs`, `cargo fmt --check`, `cargo clippy`, `cargo test`, `cargo build --release`)
and the pull request itself — which per the design is opened **only** when the report is complete,
assigned to `ECiurleo` and never merged by the auditor. The README documentation-table row the design's
PR step names is already added on this branch, pointing at this file next to its S3 and S4 siblings.
