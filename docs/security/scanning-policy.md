# Security Scanning and Dependency Policy

## Overview

This document defines the security scanning and dependency management policy for Tucano Test API.

## CI Security Scanning

The following security scans run automatically on every PR and push to main:

### 1. Dependency Audit (`cargo audit`)
- Scans Rust dependencies for known vulnerabilities
- Uses the RustSec Advisory Database
- **Fails the build** if any known vulnerabilities are found

### 2. Secret Scanning (`gitleaks`)
- Scans the **full commit history** with gitleaks in git mode over a full-depth
  (`fetch-depth: 0`) checkout: a secret that a later commit deleted is still detected, because
  `git clone` delivers history along with the tip — the gap the S4 audit recorded as F-179-5 and
  #334 closed
- Detects API keys, passwords, tokens, and other sensitive data
- **Fails the build** if any secrets are detected
- The rules are the pinned image's default set plus the committed `.gitleaks.toml`, whose
  `[allowlist]` holds a handful of exact strings, each a human-reviewed synthetic fixture
  already in history (unit-test constants, a fake PEM in a documentation recipe, an
  audit-report sentinel). Allow-listing is per *value*, never per path or commit, and only
  after the string is confirmed not to be a real credential
- A value that a security report needs to quote verbatim, such as a probe's sentinel, should
  still be written in a form the detector's entropy rule does not mistake for a credential:
  an allow-list entry is the reviewed record of a fixture that already reached `main`, not a
  shortcut for shipping a new credential-shaped string
- The same applies to this policy and its instructions: a credential shape written literally
  into a tracked document — including one quoted as an example of what to scan for — is a
  finding, so examples assemble the shape from parts instead

### 3. Container Image Scanning (`trivy`)

Two jobs scan the image, under one policy: report CRITICAL and HIGH severity issues, ignore
findings with no available fix, and **fail the run** if anything is found.

- **Every pull request, every push to `main`, and weekly** — `container-scan` in `security.yml`
  builds the image inside the job (`docker build --file Dockerfile --tag tucano-test .`) and scans
  that local build.
- **Every push to `main` and every `v*.*.*` tag, in the job that publishes** — `release.yml` scans
  the artifact it just pushed, and only that artifact: the image reference is assembled from the
  build step's `digest` output, so the scan is tied to one content-addressed digest and never to a
  re-resolved moving tag. The job fails on a finding the same way, so the digest a consumer pulls
  is the digest a passing scan reported on.

The release scan runs after the push by design, so a failing scan fails the release run but does not
withdraw the digest from the registry. Promotion and rollback therefore gate on a green release run
for the tag they deploy, not on the tag's mere presence
(see [canary-validation-and-rollback.md](../deployment/canary-validation-and-rollback.md)).

### 4. SBOM Generation (`cargo-cyclonedx`)
- Generates a Software Bill of Materials (SBOM) in CycloneDX JSON format
- Uploaded as a build artifact (`sbom.json`) for compliance and auditing
- The job parses the document before uploading it and **fails** if it is missing,
  empty, malformed, or lists no components, so an empty generation can never be
  published as a successful artifact
- Provides complete dependency inventory for security reviews

## Dependency Management

### Version Pinning
- Version requirements for direct dependencies are declared in `Cargo.toml`; the committed
  `Cargo.lock` is what fixes the versions a build resolves, transitive crates included. No
  requirement is an exact pin: Cargo derives a caret range from a bare `version`, so
  `serde = "1.0.229"` means `>=1.0.229, <2.0.0`, and no dependency uses the `=` operator.
- The `[dependencies]` block (`Cargo.toml:8-23`) mixes precisions. Eight crates name a full
  `major.minor.patch` version — `serde`, `serde_json`, `axum`, `tokio`, `tower-http`,
  `tracing-subscriber`, `fs2`, `roxmltree` — which sets the range's floor at that patch release,
  while eight name only a major/minor line — `tracing = "0.1"`, `aes-gcm = "0.11"`,
  `argon2 = "0.5"`, `base64 = "0.22"`, `getrandom = "0.2"`, `hmac = "0.12"`, `rand = "0.8"`,
  `sha2 = "0.10"` — which sets the floor at that line's `.0` release. Every `[dev-dependencies]`
  entry (`Cargo.toml:26-30`) names a full `major.minor.patch` version.
- Because every requirement is a range, a newer release can satisfy it and `cargo update` can move
  a resolved version with no `Cargo.toml` change; the `Cargo.lock` diff is the review signal.
- Use `cargo update` deliberately and review changes before committing
- Lock file (`Cargo.lock`) is committed to ensure reproducible builds

### Adding New Dependencies
1. Evaluate necessity - can existing dependencies handle this?
2. Check maintenance status - is the crate actively maintained?
3. Review security history - any past vulnerabilities?
4. Check license compatibility - must be compatible with MPL-2.0
5. Document justification in the PR description

### Updating Dependencies
1. Run `cargo update` to get latest compatible versions
2. Run `cargo audit` to check for new vulnerabilities
3. Run full test suite to verify compatibility
4. Review changelog for breaking changes
5. Update `Cargo.toml` if major version change is needed

## Unsafe Code Policy

**Unsafe Rust code is forbidden** in this project.

The crate lint configuration enforces this:
```toml
[lints.rust]
unsafe_code = "forbid"
```

Any exception requires:
1. Explicit documentation of why unsafe is necessary
2. Security review from at least one other team member
3. Comprehensive tests proving safety invariants
4. Approval from project maintainer

## Vulnerability Response

When a vulnerability is discovered:

1. **Immediate**: Assess severity and exploitability
2. **Critical/High**: Patch within 24 hours
3. **Medium**: Patch within 7 days
4. **Low**: Patch within 30 days

### Patching Process
1. Update the vulnerable dependency
2. Run `cargo audit` to verify fix
3. Run full test suite
4. Deploy security patch
5. Document in security incident log

## Testing Security Controls

To verify CI security controls are working, run the job's own body. For the secret scan the
fixture must be inside a **commit** — git mode reads history, and the working tree alone proves
nothing. Do it on a scratch clone or a scratch branch, never on anything that reaches `main`:

```bash
# Test dependency audit
cargo audit

# Test secret scanning (should detect the fixture). The fixture must be a
# credential shape the pinned detector's rule set still ships: the well-known
# AWS example key this used to quote is not detected by gitleaks v8.9.0, so a
# test with it exited 0 while proving nothing.
# Assemble the header from parts rather than writing it out in a tracked
# document: this file itself is tracked, and git mode would now flag a literal
# credential shape anywhere in history.
HEADER='-----BEGIN RSA PRIVATE KEY'"${EMPTY}"''
printf '%s\n' "$HEADER" 'MIIEowIBAAKCAQEA1234567890abcdefghijklmnopqrstuvwxyz' '-----END RSA PRIVATE KEY-----' > tracked_secret_fixture.txt
git add tracked_secret_fixture.txt
git commit -m "scratch: prove the secret scan fails"
git rm tracked_secret_fixture.txt
git commit -m "scratch: delete it again"
docker run --rm -v "$PWD:/repo:ro" zricethezav/gitleaks:v8.9.0 detect --source /repo --redact
# expect: WRN leaks found: 1 and a non-zero exit — the file is gone from every
# tree but lives in the two scratch commits, which is exactly what the old
# --no-git mode could not see and what F-179-5 (#334) closed.

# Test container scanning (the security.yml job's local build)
docker build -t tucano-test .
trivy image tucano-test

# Test the release scan: name a published digest, never a tag, and use the
# policy the job uses. The reference is what `release.yml` builds from the
# build step's `digest` output.
trivy image --exit-code 1 --ignore-unfixed --severity CRITICAL,HIGH \
  ghcr.io/tucanotechnology/tucanotestapi@sha256:<digest>

# Clean up
git reset --soft HEAD~1
rm tracked_secret_fixture.txt "$SCAN_DIR"
```

## Compliance

- SBOM artifacts are retained for 90 days
- Security scan results are available in GitHub Actions logs
- Vulnerability disclosures should be sent to security@tucano.example.com
