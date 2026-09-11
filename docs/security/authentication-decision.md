# Authentication Decision (Issue #107)

Issue: [#107](https://github.com/TucanoTechnology/TucanoTestAPI/issues/107) — make and document the
authentication decision. Split from [#14](https://github.com/TucanoTechnology/TucanoTestAPI/issues/14)
(operational hardening). This document is the decision of record; implementation is tracked separately.

## Decision

**The approach is fixed, and it is now implemented.** This issue (#107) chose the approach and wrote no
code; the implementation landed separately under [#130](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130),
following the decisions below.

| Aspect | Decision |
| --- | --- |
| Timing | **Implemented** in [#130](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130), gated by `TUCANO_AUTH_REQUIRED` (default `false`, so an existing deployment is unchanged until it opts in). |
| Authorization model | **Project-scoped RBAC.** Permissions and access are granted **per project by role** (`viewer` < `editor` < `owner`), with a `systemAdmin` global bypass. The project is the authorization boundary; a role is scoped to a project, not global. |
| Authentication mechanism | **Short-lived JWT access token plus a refresh token.** Stateless and database-free. |
| Persistence | **No database.** Principals and their per-project grants live under `TUCANO_DATA_DIR/auth/` as JSON; the file-based deployment and its persistent volume remain the only state. |
| Authentication middleware | **Decided: no middleware.** Enforcement is a `Principal` request extractor plus per-handler guards, so an unauthenticated request is refused before the body is read and the route table stays the single place a caller's reach is decided. |

The intended balance is: no database, no per-request file I/O for the common case, and revocation risk bounded
by the access-token expiry window, with refresh handling carrying the revocation window.

## Current state (as of 2026-09-11)

- Authentication and authorization are **implemented** on `main`: user, role, and token types live under
  `src/auth/`, and every guarded handler declares a `Principal` extractor and enforces project-scoped RBAC
  through the guards in `src/api/access.rs`.
- `openapi.json` publishes a global `security: [{"bearerAuth": []}]` requirement plus a
  `components.securitySchemes.bearerAuth` HTTP `bearer` scheme, and marks the five operations that need no
  caller (`/health`, `/openapi.json`, `/api-docs`, `/auth/login`, `/auth/refresh`) with `security: []`.
- The [threat model](threat-model.md) lists "Unauthorized access" as implemented, verified by the authorization
  matrix in `tests/auth.rs` plus the hand-marked public surface check in `tests/service.rs`.
- The architecture record already places an "authentication boundary" and "authorization" in the target
  architecture ([rust-service-core.md](../architecture/rust-service-core.md)).

## Rationale

- **No identity to authenticate yet, when the decision was made.** At the time of #14/#107 the API had no users
  or roles and the primary consumer (the GUI) had not defined a user model, so building hooks speculatively was
  out of scope. #130 implemented the approach once the shape was settled, adding the user, role, and token
  types under `src/auth/`.
- **The no-database constraint drives the mechanism.** A stateless JWT avoids a server-side session store and
  per-request file reads, which keeps the file-based, portable deployment model intact.
- **Short expiry bounds the revocation trade-off.** Stateless tokens cannot be revoked instantly; a short
  access-token lifetime keeps that window small without a database, and the refresh token carries renewal.
- **The project is the natural boundary.** Suites and cases live inside projects (the real-home tree of #65), so
  authorizing per project by role maps onto the storage model with no new top-level concept.

## Requirements this places on the implementation

1. No database, and no per-replica session state — consistent with `AGENTS.md` and the stateless-replica model.
2. Principals and their per-project roles must be stored without a database; if any legacy Draft 2020-12 schema
   gains a field, that is a breaking change and needs its own versioning plan before code.
3. The security scheme must be published in `openapi.json` and usable through Swagger UI; the document now
   publishes `bearerAuth` as a global requirement, with the five caller-free operations marked `security: []`.
4. The "Unauthorized access" abuse case in the [threat model](threat-model.md) must gain its auth-matrix tests.
5. The API must **not** be exposed beyond a trusted network until this is implemented. Enforcement is now in
   place; exposure still requires `TUCANO_AUTH_REQUIRED=true` plus a `TUCANO_JWT_SECRET` of at least 32 bytes,
   and `docs/security/threat-model.md` should be re-read before any deployment that faces untrusted networks.

## Tracking

Implementation is tracked in **[#130 — Auth: implement authentication](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130)**,
which carries this approach as its description, alongside the parent hardening ticket #14.

Implementation landed on `feat/p2-130-auth`. What it publishes:

- `openapi.json` declares `components.securitySchemes.bearerAuth` — an HTTP `bearer` scheme carrying a JWT
  `bearerFormat` — as a global `security` requirement, and marks the five operations that need no caller
  (`GET /health`, `GET /openapi.json`, `GET /api-docs`, `POST /auth/login`, `POST /auth/refresh`) with
  `security: []`.
- Four session endpoints exist: `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`, and
  `GET /auth/me`. Success answers `SessionResponse` (access token, refresh token, token type, and the access
  token's lifetime in seconds) or `MeResponse` (the caller's identity plus its per-project roles). Failure
  answers `401` with a `WWW-Authenticate: Bearer` challenge and one of the `missing_token`, `invalid_token`,
  `token_expired`, `invalid_credentials`, or `invalid_refresh_token` codes.
- Enforcement is wired: every guarded handler declares a `Principal` extractor, which refuses an unauthenticated
  request before the body is read, and then calls the matching guard in `src/api/access.rs` to authorize the
  caller by project and role. Listings are filtered to the projects the caller reaches rather than refused;
  creating or duplicating a project requires `systemAdmin`; a milestone must name a project-bearing reference.
  The surface is pinned by the authorization matrix in `tests/auth.rs` and the security-posture check in
  `tests/service.rs`.
- The configuration is inert until opted into: `TUCANO_AUTH_REQUIRED` defaults to `false`, so an existing
  deployment behaves exactly as before. Turning it on also requires `TUCANO_JWT_SECRET`
  (or `TUCANO_JWT_SECRET_FILE`) of at least 32 bytes, and optional `TUCANO_ACCESS_TOKEN_TTL` /
  `TUCANO_REFRESH_TOKEN_TTL` / `TUCANO_BOOTSTRAP_USERNAME` / `TUCANO_BOOTSTRAP_PASSWORD`.

Known gaps are recorded in the [threat model](threat-model.md) under "Known limitations": there is no
grant-administration endpoint yet, so accounts and grants are provisioned out of band.
