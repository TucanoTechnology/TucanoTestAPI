# Configuration Reference (Issue #190)

Issue: [#190](https://github.com/TucanoTechnology/TucanoTestAPI/issues/190) — one table of every
setting: name, environment variable, configuration file key, default, and security sensitivity.

This document is the single source of truth for precedence. The decision record is
[`docs/security/configuration-decision.md`](../security/configuration-decision.md); this page
lists every setting it governs.

## Precedence

For every setting below, the effective value is resolved **per key** in this order:

1. The **environment variable**, if set.
2. The **configuration file** key, if present.
3. The **built-in default**, if one exists.

A deployment that sets one value in the file and another in the environment gets both: each
setting's winner is decided independently. The configuration file is optional — an absent file
(`TUCANO_CONFIG_FILE` unset) preserves the pre-file behaviour exactly.

## Environment-only settings

These settings are read from the environment at startup only and have no file equivalent. The first
four must be readable *before* the configuration file can be located or its secrets decrypted; the
rest are not part of the file's key map at all.

| Setting | Environment variable | Default | Sensitivity | Notes |
| --- | --- | --- | --- | --- |
| Data directory | `TUCANO_DATA_DIR` | `data` | none | Decides where the data volume is. Must be settable by the orchestrator before any file can be read. |
| Listener port | `PORT` | `3000` | none | Owned by the orchestrator. |
| Configuration file path | `TUCANO_CONFIG_FILE` | *(unset — no file)* | none | Names the optional JSON configuration file. Leaving it unset is the documented "no file" case. |
| Key file path | `TUCANO_CONFIG_KEY_FILE` | *(unset — no keys)* | low | Names the optional key ring file for decrypting AEAD-encrypted secrets in the configuration file. |
| Advisory lock timeout | `TUCANO_LOCK_TIMEOUT_MS` | `5000` | none | Milliseconds a write waits for the shared volume's advisory lock before it is refused with the `503` `lock_timeout` answer. Must be a `u64`; an unparseable value stops startup, while an empty value keeps the default, exactly as an unset one does. |
| Request body cap | `TUCANO_MAX_BODY_BYTES` | `52428800` (50 MiB) | none | Largest request body the router accepts, in bytes; an over-limit request is refused before its body is read. Environment-only like the lock timeout; a whole number greater than zero, anything else stops startup (#103). |
| Request timeout | `TUCANO_REQUEST_TIMEOUT_MS` | `300000` (five minutes) | none | Wall clock a request may take before it is cut off with the `504` `request_timeout` answer; an explicit `0` disables the deadline for a deployment that means it. A whole number; anything else stops startup (#103). |
| In-flight cap | `TUCANO_MAX_CONCURRENCY` | `128` | none | Requests allowed in flight before new ones are refused with the `503` `service_unavailable` answer and a `Retry-After`; the server refuses rather than queues. An explicit `0` removes the cap (#103). |
| Log filter | `TUCANO_LOG` | `info` | none | The `tracing-subscriber` directive set the process subscriber is built from. An unset, empty or blank value keeps `info` — the request spans, the audit lines and the failures, without the per-connection noise `debug` adds. A directive set `tracing-subscriber` rejects stops startup rather than a request. |
| Log format | `TUCANO_LOG_FORMAT` | `compact` | none | `compact` renders one human-readable line per event, coloured only when stdout is a terminal so a captured log never carries escape sequences; `json` renders one object per event, uncoloured, for a log collector. Any other value stops startup. |

## File and environment settings

These settings may come from the environment, the configuration file, or the built-in default.

| Setting | Environment variable | File key | Default | Sensitivity | Notes |
| --- | --- | --- | --- | --- | --- |
| Auth required | `TUCANO_AUTH_REQUIRED` | `auth_required` | `false` | none | `true`, `false`, `1`, `0`, `yes`, `no`, `on`, `off`. |
| JWT signing secret | `TUCANO_JWT_SECRET` | `jwt_secret` | *(unset)* | **secret** | Inline HS256 signing key. At least 32 bytes required when auth is enabled. May be an AEAD-encrypted envelope in the file. |
| JWT secret file | `TUCANO_JWT_SECRET_FILE` | `jwt_secret_file` | *(unset)* | path | Path to a file holding the signing secret. Cannot be set together with `jwt_secret`. |
| Access token TTL | `TUCANO_ACCESS_TOKEN_TTL` | `access_token_ttl` | `15m` | none | Duration: bare seconds, or `s`, `m`, `h`, `d` suffix. Must be non-zero. |
| Refresh token TTL | `TUCANO_REFRESH_TOKEN_TTL` | `refresh_token_ttl` | `14d` | none | Duration: same format as access token TTL. Must be non-zero. |
| Bootstrap username | `TUCANO_BOOTSTRAP_USERNAME` | `bootstrap_username` | *(unset)* | low | Account created at startup when the store holds no accounts. Must be paired with the password. |
| Bootstrap password | `TUCANO_BOOTSTRAP_PASSWORD` | `bootstrap_password` | *(unset)* | **secret** | Password for the bootstrap account. Must be paired with the username. May be an AEAD-encrypted envelope in the file. |

Defaults here are the **service's** built-in defaults. The shipped `docker-compose.yml` overrides
one of them — it sets `TUCANO_AUTH_REQUIRED=true` and reads the signing secret and bootstrap pair
from `.env` — so the local Compose stack is authenticated while the service itself still starts
anonymous when the variable is unset.

## Validation rules

The following combinations are rejected at startup with a clear error message. Errors name the
*setting* at fault, never the *value*.

| Rule | Error | Rationale |
| --- | --- | --- |
| Auth enabled but no secret configured | `MissingSecret` | A server that enforces auth cannot sign tokens without a key. |
| Both `jwt_secret` and `jwt_secret_file` set | `SecretSourcesConflict` | Ambiguous: which source is authoritative? Pick one. |
| Signing secret shorter than 32 bytes | `ShortSecret` | A key shorter than the HMAC-SHA256 digest weakens the signature. |
| Only one of bootstrap username/password set | `IncompleteBootstrap` | A half-pair is always a typo. |
| TTL is empty, zero, or unparseable | `InvalidTtl` | A token that expires on issue is always a mistake. |
| `auth_required` is not a recognised boolean | `InvalidBool` | Typos like `yes_please` must fail rather than default to `false`. |
| Configuration file version is not `1` | `UnsupportedVersion` | A future schema must not be half-read. |
| Configuration file contains an unknown key | `Malformed` | A typo in a key name must not silently do nothing. |
| Encrypted secret with no matching key in the ring | `DecryptionFailed` | Fail closed: never start believing a secret was read when it was not. |
| Encrypted secret with corrupted ciphertext | `DecryptionFailed` | GCM tag mismatch: wrong key or tampered data. |

## Configuration file format

The file is a strict JSON document. Unknown keys are rejected. The `version` field is mandatory.

```json
{
  "version": 1,
  "auth_required": true,
  "jwt_secret_file": "/run/secrets/jwt",
  "access_token_ttl": "15m",
  "refresh_token_ttl": "14d",
  "bootstrap_username": "admin"
}
```

The signing secret here uses the file path form (`jwt_secret_file`) rather than an inline
value; the bootstrap password is supplied through the environment or as an encrypted
envelope. See [Encrypted secrets](#encrypted-secrets) below and the shipped
[`config.example.json`](config.example.json) for the full set of fields.

### Encrypted secrets

Secret fields (`jwt_secret`, `bootstrap_password`) may carry an AEAD-encrypted envelope instead
of a plain string. The envelope is a JSON object:

```json
{
  "version": 1,
  "key_id": "key-2026-09",
  "algorithm": "aes-256-gcm",
  "nonce": "<base64url, 12 bytes>",
  "ciphertext": "<base64url, ciphertext + GCM auth tag>"
}
```

The key ring is a separate JSON file named by `TUCANO_CONFIG_KEY_FILE`:

```json
{
  "keys": [
    { "id": "key-2026-09", "key": "<base64url, 32 bytes>" }
  ]
}
```

Multiple keys coexist during rotation. See
[`docs/security/configuration-decision.md`](../security/configuration-decision.md#key-management)
for the rotation procedure.

## Security sensitivity legend

| Level | Meaning |
| --- | --- |
| **secret** | Must never appear in logs, error messages, responses, or diagnostics. May be AEAD-encrypted in the file. |
| path | A filesystem path — not itself sensitive, but reveals deployment layout. Never echoed in errors. |
| low | Operationally useful but not exploitable on its own. |
| none | No security impact if disclosed. |
