#!/usr/bin/env bash
#
# Contract-coverage check for the HTTP API.
#
# Usage:
#   scripts/coverage-check.sh
#   COVERAGE_LOG=/tmp/hits.tsv scripts/coverage-check.sh
#
# The suites prove the contract two ways. A static table in `tests/service.rs`
# pairs every documented operation in `openapi.json` with a covering test, and
# this script adds the half a table cannot give: the router records the template
# of every request that reached a route while the suites ran, and
# `tests/route_coverage.rs` then demands a successful answer for each documented
# operation. An operation nothing drove, or that only ever answered an error,
# fails the check.
#
# It takes two passes because libtest gives no order between test binaries: the
# first names the recording and lets the suites fill it, the second reads it.
# Recording is off during the second pass — it fetches the document through the
# same probed router, and letting that land in the file would let a rerun answer
# for `get /openapi.json` out of the checker instead of out of a suite. The
# recording goes to COVERAGE_LOG (default target/route-coverage/hits.tsv) and is
# truncated first, so a stale recording from an earlier run can never stand in
# for a run that covered less.
#
# Requires: cargo. The first pass takes as long as the suite normally does.

set -euo pipefail

cd "$(dirname "$0")/.."

LOG="${COVERAGE_LOG:-$PWD/target/route-coverage/hits.tsv}"
mkdir -p "$(dirname "$LOG")"
: >"$LOG"

echo "coverage-check: recording to $LOG"
TUCANO_ROUTE_LOG="$LOG" cargo test --all-targets --all-features

echo "coverage-check: checking the recording"
# --nocapture so the summary line — how many of the documented operations were
# reached, and how many answered successfully — is visible on a green run.
TUCANO_ROUTE_LOG="$LOG" TUCANO_ROUTE_LOG_ASSERT=1 cargo test --test route_coverage -- --nocapture
