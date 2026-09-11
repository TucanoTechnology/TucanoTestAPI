# Security Test Fixtures

This directory contains negative security test fixtures for the threat model.

## Fixture Categories

### Path Traversal Tests
- `endpoints/traversal-dot-dot.json` - Attempts with `..` sequences

### Malicious JSON Tests
- `endpoints/malformed-json.json` - Invalid JSON syntax

## Usage

The security tests are implemented in `tests/security_tests.rs` and construct their
traversal, symlink, and malformed-input cases inline. These fixture files are
illustrative negative payloads rather than inputs the test suite reads; they document
the shapes the controls reject.
