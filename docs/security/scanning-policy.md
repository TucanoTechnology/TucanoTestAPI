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
- Scans the tracked tree for accidentally committed secrets — every file `git ls-files`
  reports, extracted with `git archive` and scanned from a temporary directory so that the
  container's own uid can always read it
- Detects API keys, passwords, tokens, and other sensitive data
- **Fails the build** if any secrets are detected
- Does **not** walk commit history, and does not scan untracked or ignored files. A secret
  committed and later deleted still lives in history, where `git clone` delivers it; closing
  that gap is [finding F-179-5](audit-s4-dependencies-and-supply-chain.md#f-179-5-run-the-secret-scan-over-history-or-correct-the-claim-that-it-does)
  in the S4 audit report
- A value that a security report needs to quote verbatim, such as a probe's sentinel, must be
  written in a form the detector's entropy rule does not mistake for a credential; a flagged
  sentinel is a false positive to be reworded, not a leak to be allowlisted
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


CI supply-chain pins (#333): every `uses:` call site in `.github/workflows/` references a
full commit SHA with the release tag retained as a trailing comment, every workflow
container image and `docker run` reference carries its manifest digest, and the
Dockerfile bases are digest-pinned. Dependabot (`.github/dependabot.yml`) opens weekly
bump pull requests for actions, images, and crates; the pins only move when a bump PR
passes the full suite and merges — no silent drift, and no rot. One residual trust:
`trivy-action` downloads its scanner binary from the action's own pinned release at run
time, so the action SHA pins the workflow logic while the trivy build artifact arrives over
TLS from the same repository whose commit is pinned here.
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
fixture must be **inside the tracked tree** — an untracked file is deliberately not scanned —
and it must be removed before committing:

```bash
# Test dependency audit
cargo audit

# Test secret scanning (should detect the fixture)
# The fixture must be a credential shape the pinned detector's rule set still
# ships. The well-known AWS example key this used to quote
# (`wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY`) is not detected by gitleaks
# v8.9.0, so that "test" exited 0 while proving nothing.
#
# Build it from parts rather than writing the header out here: this document is
# itself tracked, so a literal credential shape in the prose above makes the
# scan fail on this file. An assembled one has no single line for the rule to
# match, while the fixture it writes still does.
HEADER='-----BEGIN RSA PRIVATE KEY'"${EMPTY}"''
printf '%s\n' "$HEADER" 'MIIEowIBAAKCAQEA1234567890abcdefghijklmnopqrstuvwxyz' '-----END RSA PRIVATE KEY-----' > tracked_secret_fixture.txt
git add tracked_secret_fixture.txt
git commit -m "scratch: prove the secret scan fails"
SCAN_DIR="$PWD/.scan-extract"
rm -rf "$SCAN_DIR"
mkdir -p "$SCAN_DIR"
git archive --format=tar HEAD | tar -x -C "$SCAN_DIR"
chmod -R a+rX "$SCAN_DIR"
docker run --rm -v "$SCAN_DIR:/repo:ro" zricethezav/gitleaks:v8.9.0 detect --source /repo --no-git --redact
# expect: WRN leaks found: 1 and a non-zero exit
# The extraction lives under the workspace, not in $TMPDIR: the runner's Docker
# only accepts bind mounts from paths it is configured to share. `git archive`
# exports the committed tree, so the fixture has to be committed to be scanned.

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
