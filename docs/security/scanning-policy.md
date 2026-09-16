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

### 3. Container Image Scanning (`trivy`)
- Scans the production Docker image for OS and library vulnerabilities
- Reports CRITICAL and HIGH severity issues
- **Fails the build** if any CRITICAL or HIGH vulnerabilities are found

### 4. SBOM Generation (`cargo-cyclonedx`)
- Generates a Software Bill of Materials (SBOM) in CycloneDX JSON format
- Uploaded as a build artifact (`sbom.json`) for compliance and auditing
- The job parses the document before uploading it and **fails** if it is missing,
  empty, malformed, or lists no components, so an empty generation can never be
  published as a successful artifact
- Provides complete dependency inventory for security reviews

## Dependency Management

### Version Pinning
- All dependencies are pinned to specific versions in `Cargo.toml`
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
fixture must be **inside the tracked tree** — an untracked file is deliberately not scanned —
and it must be removed before committing:

```bash
# Test dependency audit
cargo audit

# Test secret scanning (should detect the fixture)
echo 'AWS_SECRET_ACCESS_KEY = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"' >> tracked_secret_fixture.txt
git add tracked_secret_fixture.txt
SCAN_DIR=$(mktemp -d)
git archive --format=tar HEAD | tar -x -C "$SCAN_DIR"
chmod -R a+rX "$SCAN_DIR"
docker run --rm -v "$SCAN_DIR:/repo:ro" zricethezav/gitleaks:v8.9.0 detect --source /repo --no-git --redact
# expect: WRN leaks found ... and a non-zero exit

# Test container scanning
docker build -t tucano-test .
trivy image tucano-test

# Clean up
git rm --cached tracked_secret_fixture.txt
rm tracked_secret_fixture.txt "$SCAN_DIR"
```

## Compliance

- SBOM artifacts are retained for 90 days
- Security scan results are available in GitHub Actions logs
- Vulnerability disclosures should be sent to security@tucano.example.com
