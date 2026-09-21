#!/usr/bin/env bash
#
# Scratch-CRUD smoke check for a running Tucano Test API (issue #96).
#
# Usage:
#   scripts/smoke.sh [BASE_URL]
#   SMOKE_BASE_URL=http://localhost:3100 scripts/smoke.sh
#
# BASE_URL is the first argument, else SMOKE_BASE_URL, else the first of
# http://localhost:3100 (the port `docker compose up` publishes) and
# http://localhost:3000 (the port `cargo run` binds) that answers `/health` —
# the same survey scripts/clear-data.mjs makes. The script checks the health
# endpoint, lists projects, creates a uniquely named scratch project with a
# scratch test case inside it, reads both back, then deletes the case and the
# project and confirms each deletion is observable. It exits non-zero on the
# first deviation and always tries to remove whatever it created, so a failing
# candidate is never left holding scratch data.
#
# Authentication (issue #195): on a deployment running with
# TUCANO_AUTH_REQUIRED=true every call below needs a caller, and creating a
# project needs a system administrator. Set SMOKE_USERNAME and SMOKE_PASSWORD
# to the bootstrap account's credentials and the script signs in through
# POST /auth/login and presents the access token on every request. Set
# SMOKE_TOKEN instead to present a ready-made bearer token. With none of them
# set the script calls the API unauthenticated, which is what a deployment with
# authentication switched off expects.
#
# Requires: curl, python3.

set -euo pipefail

for tool in curl python3; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "smoke: '$tool' is required but was not found on PATH" >&2
    exit 2
  }
done

# An explicit base URL is taken as given — probing it would hide a typo behind a
# silent fallback to a different API. Only the unset case surveys the two ports
# this repository's two ways of running the API publish.
BASE_URL="${1:-${SMOKE_BASE_URL:-}}"
if [ -z "$BASE_URL" ]; then
  for candidate in http://localhost:3100 http://localhost:3000; do
    if curl -fsS --max-time 2 "$candidate/health" >/dev/null 2>&1; then
      BASE_URL="$candidate"
      break
    fi
  done
  [ -n "$BASE_URL" ] || {
    echo "smoke: no API answered /health; tried http://localhost:3100 and http://localhost:3000" >&2
    exit 2
  }
fi
BASE_URL="${BASE_URL%/}"

SUFFIX="$(date +%s)-$$"
PROJECT_NAME="smoke-${SUFFIX}"
PROJECT_ID="${PROJECT_NAME}.json"
CASE_ID="smoke-case-${SUFFIX}"
CASE_TITLE="Smoke case ${SUFFIX}"
CASE_EXPECTED="Recorded by scripts/smoke.sh"

HTTP_STATUS=""
HTTP_BODY=""
TOKEN="${SMOKE_TOKEN:-}"

fail() {
  echo "smoke: FAIL — $*" >&2
  exit 1
}

# --- JSON helpers (python3 reads stdin) -------------------------------------

# Prints the top-level field named by $1.
json_field() {
  python3 -c 'import json, sys; print(json.load(sys.stdin)[sys.argv[1]])' "$1"
}

# Prints the top-level JSON type name (`array`, `object`, `string`, …), or
# "invalid" when stdin is not JSON.
json_type() {
  python3 -c 'import json, sys
try:
    value = json.load(sys.stdin)
except Exception:
    print("invalid")
    raise SystemExit
if isinstance(value, list):
    print("array")
elif isinstance(value, dict):
    print("object")
elif isinstance(value, bool):
    print("boolean")
elif isinstance(value, str):
    print("string")
elif value is None:
    print("null")
elif isinstance(value, (int, float)):
    print("number")
else:
    print("unknown")'
}

# Prints "yes" when the JSON array on stdin holds exactly one element equal to $1.
json_array_has() {
  python3 -c 'import json, sys; print("yes" if sys.argv[1] in json.load(sys.stdin) else "no")' "$1"
}

# Prints how many elements of the JSON array on stdin equal $1.
json_array_count() {
  python3 -c 'import json, sys; print(json.load(sys.stdin).count(sys.argv[1]))' "$1"
}

# --- HTTP helper ------------------------------------------------------------

# http <METHOD> <PATH> [JSON_BODY]; sets HTTP_STATUS and HTTP_BODY.
http() {
  local method="$1" path="$2" body="${3:-}" output status
  local -a auth=()
  [ -n "$TOKEN" ] && auth=(--header "Authorization: Bearer ${TOKEN}")
  output="$(mktemp)"
  if [ -n "$body" ]; then
    status="$(curl --silent --show-error --connect-timeout 10 --max-time 30 \
      --output "$output" --write-out '%{http_code}' \
      --request "$method" --header 'Content-Type: application/json' \
      "${auth[@]}" \
      --data "$body" "${BASE_URL}${path}")"
  else
    status="$(curl --silent --show-error --connect-timeout 10 --max-time 30 \
      --output "$output" --write-out '%{http_code}' \
      --request "$method" "${auth[@]}" "${BASE_URL}${path}")"
  fi
  HTTP_STATUS="$status"
  HTTP_BODY="$(<"$output")"
  rm -f "$output"
}

# Signs in as SMOKE_USERNAME/SMOKE_PASSWORD when SMOKE_TOKEN was not supplied.
# Reads accessToken out of the session response; the response is the only place
# a fresh token comes from, so a missing one is a hard failure rather than a
# silently unauthenticated run.
sign_in() {
  [ -n "$TOKEN" ] && return 0
  [ -n "${SMOKE_USERNAME:-}" ] || return 0
  [ -n "${SMOKE_PASSWORD:-}" ] ||
    fail "SMOKE_USERNAME is set but SMOKE_PASSWORD is not"

  local body
  body="$(printf '{"username":"%s","password":"%s"}' "$SMOKE_USERNAME" "$SMOKE_PASSWORD")"
  http POST /auth/login "$body"
  expect_status 200 "POST /auth/login"
  TOKEN="$(printf '%s' "$HTTP_BODY" | json_field accessToken)"
  [ -n "$TOKEN" ] || fail "POST /auth/login answered without an accessToken — ${HTTP_BODY}"
  echo "smoke: signed in as ${SMOKE_USERNAME}"
}

expect_status() {
  local expected="$1" what="$2"
  [ "$HTTP_STATUS" = "$expected" ] ||
    fail "$what: expected HTTP $expected, got $HTTP_STATUS — ${HTTP_BODY}"
}

expect_json_field() {
  local field="$1" expected="$2" what="$3" actual
  actual="$(printf '%s' "$HTTP_BODY" | json_field "$field")"
  [ "$actual" = "$expected" ] ||
    fail "$what: expected $field=$expected, got $field=$actual — ${HTTP_BODY}"
}

# --- Cleanup ----------------------------------------------------------------

cleanup() {
  # Best effort: a failure above may have left the scratch case or project
  # behind. Every error is ignored so the original exit code survives.
  local -a auth=()
  [ -n "$TOKEN" ] && auth=(--header "Authorization: Bearer ${TOKEN}")
  curl --silent --connect-timeout 10 --max-time 30 "${auth[@]}" \
    --request DELETE "${BASE_URL}/test_cases/${CASE_ID}" >/dev/null 2>&1 || true
  curl --silent --connect-timeout 10 --max-time 30 "${auth[@]}" \
    --request DELETE "${BASE_URL}/projects/${PROJECT_ID}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# --- Sequence ---------------------------------------------------------------

echo "smoke: target ${BASE_URL}"

sign_in

http GET /health
expect_status 200 "GET /health"
expect_json_field status ok "GET /health"

http GET /projects
expect_status 200 "GET /projects"
[ "$(printf '%s' "$HTTP_BODY" | json_type)" = "array" ] ||
  fail "GET /projects: expected a JSON array — ${HTTP_BODY}"

http POST /projects "$(printf '{"name":"%s"}' "$PROJECT_NAME")"
expect_status 201 "POST /projects"
expect_json_field id "$PROJECT_ID" "POST /projects"

http GET "/projects/${PROJECT_ID}"
expect_status 200 "GET /projects/${PROJECT_ID}"
expect_json_field name "$PROJECT_NAME" "GET /projects/${PROJECT_ID}"

http GET /projects
expect_status 200 "GET /projects (after create)"
[ "$(printf '%s' "$HTTP_BODY" | json_array_has "$PROJECT_ID")" = "yes" ] ||
  fail "GET /projects: created project ${PROJECT_ID} is not listed"

http POST "/projects/${PROJECT_ID}/test_cases" \
  "$(printf '{"testCaseId":"%s","title":"%s","expectedResult":"%s"}' \
    "$CASE_ID" "$CASE_TITLE" "$CASE_EXPECTED")"
expect_status 201 "POST /projects/${PROJECT_ID}/test_cases"
expect_json_field id "$CASE_ID" "POST /projects/${PROJECT_ID}/test_cases"

http GET "/test_cases/${CASE_ID}"
expect_status 200 "GET /test_cases/${CASE_ID}"
expect_json_field testCaseId "$CASE_ID" "GET /test_cases/${CASE_ID}"
expect_json_field title "$CASE_TITLE" "GET /test_cases/${CASE_ID}"
expect_json_field expectedResult "$CASE_EXPECTED" "GET /test_cases/${CASE_ID}"

http GET "/projects/${PROJECT_ID}/test_cases"
expect_status 200 "GET /projects/${PROJECT_ID}/test_cases"
[ "$(printf '%s' "$HTTP_BODY" | json_array_count "$CASE_ID")" = "1" ] ||
  fail "GET /projects/${PROJECT_ID}/test_cases: scratch case is not listed exactly once — ${HTTP_BODY}"

http DELETE "/test_cases/${CASE_ID}"
expect_status 200 "DELETE /test_cases/${CASE_ID}"

http GET "/test_cases/${CASE_ID}"
expect_status 404 "GET /test_cases/${CASE_ID} (after delete)"

http GET "/projects/${PROJECT_ID}/test_cases"
expect_status 200 "GET /projects/${PROJECT_ID}/test_cases (after delete)"
[ "$(printf '%s' "$HTTP_BODY" | json_array_has "$CASE_ID")" = "no" ] ||
  fail "GET /projects/${PROJECT_ID}/test_cases: deleted case ${CASE_ID} is still listed (partial write)"

http DELETE "/projects/${PROJECT_ID}"
expect_status 200 "DELETE /projects/${PROJECT_ID}"

http GET "/projects/${PROJECT_ID}"
expect_status 404 "GET /projects/${PROJECT_ID} (after delete)"

http GET /projects
expect_status 200 "GET /projects (after delete)"
[ "$(printf '%s' "$HTTP_BODY" | json_array_has "$PROJECT_ID")" = "no" ] ||
  fail "GET /projects: deleted project ${PROJECT_ID} is still listed (partial write)"

echo "smoke: PASS — health, scratch project/case CRUD, and both deletions observed"
