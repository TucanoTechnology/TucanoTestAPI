# Security Test Fixtures

This directory contains negative security test cases for the threat model.

## Fixture Categories

### Path Traversal Tests
- `traversal-dot-dot.json` - Attempts with `..` sequences
- `traversal-absolute.json` - Absolute path attempts
- `traversal-encoded.json` - URL-encoded traversal attempts

### Symlink Tests
- `symlink-escape.json` - Symlink pointing outside data root

### Malicious JSON Tests
- `malformed-json.json` - Invalid JSON syntax
- `oversized-payload.json` - Exceeds size limits
- `deeply-nested.json` - Excessive nesting depth
- `unknown-fields.json` - Fields not in schema

### Oversized Upload Tests
- `oversized-attachment.bin` - File exceeding size limit

### Concurrent Write Tests
- `concurrent-write-scenario.md` - Race condition test scenarios

## Usage

These fixtures are used by integration tests in `tests/security/` to verify
that the service correctly rejects malicious input while preserving data integrity.
