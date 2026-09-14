# Configuration Model and Secrets Decision (Issue #187)

Issue: [#187](https://github.com/TucanoTechnology/TucanoTestAPI/issues/187) — decide the
configuration model and how secrets are stored, before anything is implemented. Child of
[#168](https://github.com/TucanoTechnology/TucanoTestAPI/issues/168) (*Add a unified configuration
file with encrypted secrets*). This document is the decision of record; implementation is tracked by
the sibling children of #168, listed under [Tracking](#tracking).

It is a documentation change: it adds no route, no field, no stored document, and no code.

## The request, and the tension inside it

The epic asks for **one configuration file holding both settings and secrets** — "settings and its
secrets, so operators are not forced to spread configuration across environment variables. Any
secret the file holds must be encrypted at rest."

That request collides with two things already fixed in this repository, and the collision is the
substance of this decision:

1. **`AgentRules/security/secret-protection.md`** states the rule as "Use environment variables or
   `.env` files for secrets". `AgentRules/` is synced from TucanoAgentRules and must not be edited
   here, so a decision that moved secrets into a config file would need an upstream rule change to
   be honest about it.
2. **The container runs with a read-only root filesystem** (`read_only: true` in
   [`docker-compose.yml`](../../docker-compose.yml), `--read-only` in the promotion runbook, and the
   hardening table in [`docs/deployment/deployment-guide.md`](../deployment/deployment-guide.md)).
   Only `TUCANO_DATA_DIR` and the `/tmp` tmpfs are writable. A configuration file that a running
   process must *write* to cannot live on the immutable root filesystem, and the reason the root
   filesystem is immutable is precisely to stop a compromised process from persisting anything.

The rest of this document compares the options, decides, and records the key management the decision
implies.

## Current state

Configuration is environment variables only, read in exactly two places:

| Setting | Read by | Default |
| --- | --- | --- |
| `TUCANO_DATA_DIR` | [`src/main.rs`](../../src/main.rs) | `data` |
| `PORT` | [`src/main.rs`](../../src/main.rs) | `3000` |
| `TUCANO_AUTH_REQUIRED` | [`src/auth/config.rs`](../../src/auth/config.rs) | `false` |
| `TUCANO_JWT_SECRET` / `TUCANO_JWT_SECRET_FILE` | `src/auth/config.rs` | none; required when auth is on |
| `TUCANO_ACCESS_TOKEN_TTL` / `TUCANO_REFRESH_TOKEN_TTL` | `src/auth/config.rs` | `15m` / `14d` |
| `TUCANO_BOOTSTRAP_USERNAME` / `TUCANO_BOOTSTRAP_PASSWORD` | `src/auth/config.rs` (used by [`src/auth/bootstrap.rs`](../../src/auth/bootstrap.rs)) | none |

Three properties of that implementation matter to this decision, because they are what the file
option would have to preserve or knowingly give up:

- **The settings are resolved once, at startup, as a pure function of an environment lookup.**
  `AuthConfig::from_lookup` takes the lookup as a closure, which is why `src/auth/config.rs` carries
  the full validation matrix — the bool refusal, the duration refusal, the short-secret refusal, the
  "both secret sources" conflict, the half bootstrap pair — as ordinary unit tests with no global
  state and no process mutation. `std::env::set_var` is `unsafe` in edition 2024 and this crate
  forbids `unsafe_code`, so a file loader must keep the same shape: read the file into a value,
  then hand that value to a pure resolver.
- **Configuration errors are startup errors.** The process refuses to boot rather than coming up and
  failing strangely later.
- **Secrets are already covered by four distinct behaviours**: `*_FILE` variants
  (`TUCANO_JWT_SECRET_FILE`), a length floor (`MIN_SECRET_BYTES = 32`), an explicit conflict refusal
  when both sources are set, and an existing security invariant that credentials are never stored or
  logged in the clear.

Nothing in the current model is a secret at rest *owned by this service*: no AWS key is read today,
and every secret that exists arrives from outside. That is a fact the options below turn on.

## What is actually being configured

Before comparing storage media, it is worth separating the kinds of values that would go into a
unified file, because they do not have the same threat profile:

| Kind | Examples | Rotation | Who can read it | Threat if disclosed |
| --- | --- | --- | --- | --- |
| **Process/environment settings** | `TUCANO_DATA_DIR`, `PORT`, token lifetimes | restart | operators | little |
| **Service boot secrets** | the JWT signing secret | restart | operators | session forgery |
| **External integration credentials** | AWS keys for defect links or test-result imports | minutes to days, often automatic | operators *and* credential issuers | third-party account takeover |
| **Read-once bootstrap credentials** | `TUCANO_BOOTSTRAP_PASSWORD` | used once at first boot | operators | privilege escalation at first boot |

The fourth row is already handled the right way and is the model to generalise: the bootstrap
password is *transient*. It is consumed once, stored only as an Argon2id PHC hash, and never read
again — `ensure_bootstrap_user` returns `None` without touching the password once the store holds
any account. The file's job, for a secret like that, is to *deliver* it, not to *retain* it.

The third row is the one that actually motivates the epic. A per-replica environment variable is a
poor home for a credential that rotates on its own schedule: rotating it means recreating every
replica, and the credential is visible in `docker inspect` and in the Compose file.

## Options

Measured against the constraints above — keep env-only deployments working, keep secrets out of
logs and images, keep `read_only: true`, keep `AgentRules/security/secret-protection.md` satisfiable
— the three proposals behave as follows.

| | **A. Environment variables as-is** | **B. Config file only** | **C. Environment first, file as an optional second source** |
| --- | --- | --- | --- |
| Secrets rule (`secret-protection.md`) | Satisfied as written | **Violated** unless the rule changes upstream | Satisfied: environment stays a first-class, documented source |
| `read_only: true` | Fine: nothing is written | Fine *if* the file is mounted read-only and never rewritten, or lives under `TUCANO_DATA_DIR` with `0600` | Fine: same, and the file is optional |
| "One file, not spread across variables" | **Not achieved** | Achieved | Largely achieved, with a small env allow-list kept for the reasons below |
| Existing deployment | Unchanged | Breaks on upgrade unless a migration step is run | Unchanged: no file means today's behaviour exactly |
| Startup validation | Already pure and fully unit-tested | Requires a loader with the same purity | Same, plus precedence that must be tested per key |
| Image/secret hygiene | Secrets visible in `docker inspect` and Compose | Out of the image, out of `inspect` | Same, and the secret can also stay out of the Compose file |
| Rotation | Recreate each replica | Rewrite the file on the volume and restart | Rewrite the volume file and restart; per-replica env still available for overrides |
| Blast radius if the file leaks | — | *Every* setting and *every* secret, in one artifact | One value per key; environment-supplied values are unaffected |
| Compatibility with `PORT`/`TUCANO_DATA_DIR` | Fine | **Wrong tool**: `PORT` and `TUCANO_DATA_DIR` must be settable by the orchestrator before the file can be located | Both supported, which is the point |

### Why not A alone

A alone is where the repository is today, and it does not deliver what the epic asks for. It also
has a concrete operational weakness: rotating a credential means recreating every replica, and the
credential is readable by anyone who can run `docker inspect`, read the Compose file, or read the
process environment of the container.

### Why not B alone

B alone is the option the epic literally describes, and it fails four times over:

1. **It cannot locate itself.** `TUCANO_DATA_DIR` decides where the data volume is and `PORT` decides
   where the process listens. Both must be settable *before* the file can be read, so a file-only
   model still needs an environment allow-list — and once that allow-list exists, "file only" is
   already "both".
2. **It breaks every existing deployment.** The epic's own definition of done requires that "an
   existing env-only Compose deployment keeps working". A file-only model changes behaviour on
   upgrade unless a migration step is mandatory, which contradicts the requirement.
3. **It conflicts with the secret-protection rule.** `AgentRules/security/secret-protection.md`
   names environment variables and `.env` files. `AgentRules/` is synced from TucanoAgentRules and
   must not be edited in this repository, so B is only legitimate *after* that rule is changed
   upstream. #187 is not the place to fork the rule.
4. **It concentrates blast radius.** One artifact holding every setting and every secret is a single
   point of disclosure. The file's own protection (mode `0600`, on the volume, never in the image)
   then becomes the whole of the security story.

There is also a **security-through-obscurity trap** worth naming, because it is the likely motivation
for the encryption half of the request. If the encryption key is supplied by the environment or by a
file next to the configuration file, then an attacker who can read the configuration file can read
the key too, and the encryption protects against nothing except an accidental `git commit` or a
backup of the data volume. Encrypting the file is still worth doing — for exactly those two accidents
— but it must not be **sold as** a control that protects against an actor who can already read the
volume.

## Decision

**Option C wins. Environment variables stay the primary, authoritative configuration source, and a
single optional configuration file is added as a second source that is read once at startup.**

| Aspect | Decision |
| --- | --- |
| Configuration model | **Environment first, file second, defaults last**, with a documented precedence. The environment is applied *over* the file, per key, not per file: a file supplies whole settings, and any setting the environment also provides wins. |
| Environment allow-list | A small set of settings must remain environment-only because they are needed *before* the file can be read, and because the orchestrator owns them: `TUCANO_DATA_DIR`, `PORT`, and the path of the configuration file itself. Nothing else is env-only. |
| Config file location | Set explicitly by an environment variable. Do **not** search a default path implicitly: an implicit search means a stray file in a writable directory silently changes a production deployment. |
| Config file writability | **Read-only for the process.** The file is never created or rewritten by the running service, so `read_only: true` is preserved. The service either reads a mounted file or refuses to start. |
| Secrets in the file | **Permitted, and no secret may exist *only* in the file.** For every secret the environment can also supply, the environment wins, which keeps `AgentRules/security/secret-protection.md` satisfiable without editing it. |
| Secrets in logs, errors, and responses | Unchanged and non-negotiable: no secret, no raw file path, and no raw file contents reach a log line, an error envelope, or any response. The startup error for an unusable secret names the *setting*, never the value. |
| Stored secrets | Any secret the service *stores from* the file is persisted only in the already-hashed form the repository uses — Argon2id PHC for passwords, SHA-256 digests for refresh tokens — never in the clear. This generalises the bootstrap behaviour. |
| `.env` files | Not a supported mechanism of this service. `AgentRules/security/secret-protection.md` allows them; nothing in this repository reads one, and nothing will, because the container has no shell and no working directory to source from. `.env` stays an operator-side convenience for building Compose `environment:` blocks, never a file the service parses. |
| Schema | The file is strict: an unknown key is a **startup error**, mirroring `deny_unknown_fields` on stored documents. A silently ignored typo in a secret's name is a deployment that boots with no secret. |

Precedence, stated once, for the implementation tickets to copy verbatim:

1. The value is looked up in the **environment**.
2. If it is absent there, it is looked up in the **configuration file**.
3. If it is absent there too, the **built-in default** applies (or the setting counts as unset, if
   it has no default).

Precedence is resolved **per key**, not per source. A deployment that sets one value in the file and
another in the environment gets both.

## Key management

The epic asks how the encryption key is managed. The decision is **the key is supplied from outside,
and encryption of the configuration file is recommended but optional for the first implementation.**

| Question | Decision |
| --- | --- |
| **Where does the key live?** | Outside the artifact it protects, and outside the image. The supported form is a **file containing the key, mounted read-only** into the container, named by an environment variable — the same shape as `TUCANO_JWT_SECRET_FILE`, which the deployment guide already documents and the container's read-only root filesystem already accommodates. A Docker/Kubernetes secret mount is the reference deployment. |
| **How is it provided?** | By the operator, at deployment time, through the orchestrator's secret mechanism. There is **no key derivation from the configuration file's own contents** and **no key stored beside the file**: a key that travels with the ciphertext protects nothing against the actor we are defending against. |
| **What algorithm?** | Authenticated encryption only — AES-256-GCM or ChaCha20-Poly1305. Encryption without authentication would let a modified file decrypt to attacker-chosen values. The envelope must be **versioned and self-describing** (algorithm, nonce, and a key identifier) so the algorithm can change without a flag day. |
| **What happens when the key is absent?** | Depends on whether the file claims to hold secrets, and it is a **startup error either way** — never a warning, never a silent fallback to plaintext. If the file is present, marked as containing secrets, and no key is available, the service **refuses to boot**. If the key is present but the file is not marked as encrypted, the key is unused, not an error. The service must never come up believing it read a secret it did not. |
| **When is it decrypted?** | **Once, at startup, into memory**, exactly like every other setting. The decrypted value is never written back to disk, never cached in the data directory, and never included in an error. Decryption happens before the listener binds, so a failure is a startup failure. |
| **Rotation** | Two-step rotation. The envelope carries a **key identifier**, and the loader accepts a *key ring* (the active key plus retired keys still needed to read the current file). Rotating is therefore: (1) write the new key alongside the old, restart, (2) re-encrypt the file under the new key identifier, restart, (3) retire the old key. Rotation is an **operator action against the mounted file**, not a service API — this service does not rewrite configuration. There is no in-process hot reload: rotation is observed at the next restart. |
| **When a key is retired or lost** | A file whose key identifier is not in the key ring is a startup error naming the identifier, never a partial read. Losing a key means the file must be re-created from the operator's secret store; there is no recovery path inside the service, and none should be built. |

**Explicitly deferred, with rationale.**

- **A key-management service (KMS) integration** — deferred. It would be the right answer for a
  hosted deployment and it is a dependency and a trust relationship the project has not taken. The
  file-plus-read-only-mount shape works with any KMS that can materialise a file, so choosing one now
  would foreclose nothing and commit us to a vendor.
- **Automatic, periodic re-encryption** — deferred. Rotation is driven by the operator; a service
  that rewrites its own configuration fights the read-only root filesystem and the "the API is the
  only actor below `TUCANO_DATA_DIR`" rule.
- **Encryption at rest of everything under `TUCANO_DATA_DIR`** — out of scope and not implied by this
  ticket. It would change the storage concept, the inspectability guarantee that justifies the
  file-based design, and the backup/restore model.
- **Making the configuration file required** — deferred. It becomes a candidate once every
  env-only deployment has had a release to migrate; until then, optional is what keeps the epic's
  "keeps working" requirement true.
- **A `.env` parser** — rejected rather than deferred. See the decision table above.

## Threat model impact

The decision opens one new trust boundary and closes a smaller one. Both are recorded in
[`docs/security/threat-model.md`](threat-model.md); in summary:

- **New boundary — configuration file to service startup.** The file and the key file are
  **untrusted input at startup**: a malformed, unknown-keyed, tampered, or wrongly-encrypted file
  must fail closed with a startup error that names the setting and never the value. This is the same
  posture as malformed JSON from a client, moved one step earlier in the process's life.
- **New boundary — key file to service startup.** The key is the one thing whose disclosure
  defeats the file's encryption. It must never be logged, echoed in an error, or included in any
  response, and it must not be readable from the image.
- **Closed weakness — secrets in `docker inspect`.** A secret supplied through the file rather than
  through `environment:` is no longer readable by anyone who can inspect the container or read the
  Compose file. It is *not* a protection against an actor who can read the mounted volume; the
  threat model says so explicitly, so nobody mistakes file encryption for a control it is not.

## Accuracy against the implementation

The ticket asks that the decision be re-checked against the code by the ticket owner. As of
2026-09-14, on `main`:

- Configuration is env-only, read in `src/main.rs` and `src/auth/config.rs`; there is no file loader
  and no `TUCANO_CONFIG_FILE` variable. **Deliberate: this ticket ships no code.** The siblings in
  #168 are what make the decision true.
- The env allow-list in the decision (`TUCANO_DATA_DIR`, `PORT`, plus the file path) matches exactly
  what `src/main.rs` reads today.
- The four secret behaviours the file loader must preserve — `*_FILE` variants, `MIN_SECRET_BYTES`,
  the both-sources conflict, and the never-store-in-the-clear invariant — are all present as
  described, in `src/auth/config.rs`, `src/auth/store.rs`, and invariant 7 of the threat model.
- The read-only root filesystem and the writable paths (`/data`, `/tmp`) are exactly as the
  deployment guide describes.
- `AgentRules/security/secret-protection.md` says what this document quotes it as saying.

Nothing in this document contradicts the current implementation; it constrains the ones that follow.

## Consequences for the sibling tickets

| Ticket | What this decision fixes for it |
| --- | --- |
| [#188](https://github.com/TucanoTechnology/TucanoTestAPI/issues/188) — schema and loader | Optional file; explicit path from the environment; strict schema with unknown keys refused; loaded once at startup into an immutable value; whole-file read validated by the same pure resolver shape `AuthConfig::from_lookup` already uses. |
| [#189](https://github.com/TucanoTechnology/TucanoTestAPI/issues/189) — encrypted secrets at rest | AEAD envelope with a key identifier; key from a read-only mounted file named by an environment variable; fail closed when a secret-bearing file has no usable key; no key stored beside the file; no plaintext fallback. |
| [#190](https://github.com/TucanoTechnology/TucanoTestAPI/issues/190) — precedence and validation | Environment > file > default, **per key**; the three-item env allow-list; per-key precedence tests plus a test per validation refusal; a test that no secret value appears in a startup error or a log line. |
| [#191](https://github.com/TucanoTechnology/TucanoTestAPI/issues/191) — reference and migration | The precedence table above verbatim, the rotation procedure, and an env-only → file migration path that requires no change to an existing deployment. |

## Tracking

- Parent epic: [#168](https://github.com/TucanoTechnology/TucanoTestAPI/issues/168).
- This decision: [#187](https://github.com/TucanoTechnology/TucanoTestAPI/issues/187).
- Implementation children: [#188](https://github.com/TucanoTechnology/TucanoTestAPI/issues/188),
  [#189](https://github.com/TucanoTechnology/TucanoTestAPI/issues/189),
  [#190](https://github.com/TucanoTechnology/TucanoTestAPI/issues/190),
  [#191](https://github.com/TucanoTechnology/TucanoTestAPI/issues/191).

## Upstream follow-up (not part of this ticket)

`AgentRules/security/secret-protection.md` currently permits secrets only in environment variables
and `.env` files. This decision keeps that rule satisfiable by keeping the environment authoritative,
but the rule is narrower than the configuration model the epic is building. Widening it — for
example to "environment variables, a mounted secret file, or an encrypted configuration file with an
externally supplied key" — is a change to
[TucanoAgentRules](https://github.com/TucanoTechnology/TucanoAgentRules), which is the single source
of truth for `AgentRules/`. **Do not edit `AgentRules/` in this repository.** The change is listed
here so it is not lost, and it is not a prerequisite for the sibling tickets: nothing in this
decision depends on it.
