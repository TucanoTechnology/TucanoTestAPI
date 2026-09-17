#!/usr/bin/env bash
#
# The one-command demo and fixture path (issue #195).
#
# Brings a throwaway Tucano Test API stack up, authenticates it, seeds the demo
# dataset, runs the scratch-CRUD smoke check, and validates that the dataset the
# smoke check just touched really is the one the specification describes. One
# command, one exit status: zero means every stage held.
#
#   scripts/demo.sh            up -> seed -> smoke -> validate
#   scripts/demo.sh --down     tear the dataset down, then remove the stack
#   scripts/demo.sh --status   report what the stack is and whether it is up
#
# Why the Compose file is generated rather than committed
# -------------------------------------------------------
# Seeding needs an auth-enforcing deployment: scripts/seed.mjs signs in as the
# bootstrap account, and scripts/validate-seed.mjs proves an unprivileged
# session is refused a guarded write. The committed docker-compose.yml already
# enforces auth, but the demo needs its own port, Compose project name and admin
# password, so this script writes a demo copy of it into a temporary directory
# with those injected. The committed file is never modified, no second Compose
# file joins the repository, and the demo stack stays separate from the one
# `docker compose up -d --build` starts.
#
# The stack is a separate Compose project (tucano-test-demo by default) on its
# own host port, so it never touches the long-running `tucano-test` stack or its
# api container. Data lives in a named volume, so it survives a restart of the
# demo stack and is only removed by `--down`.
#
# Usage:
#   scripts/demo.sh [--down|--status] [--build] [--port N] [--project NAME]
#
# Environment:
#   DEMO_PROJECT        Compose project name (default tucano-test-demo)
#   DEMO_PORT           Host port to publish (default 3199)
#   DEMO_DATA_VOLUME    Named volume for /data (default <project>_data)
#   DEMO_ADMIN_USER     Bootstrap account to seed and validate with (default admin)
#   DEMO_ADMIN_PASSWORD Its password (default demo-admin-password)
#
# Requires: docker (with the compose plugin), node, curl, python3.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${HERE}/.." && pwd)"

PROJECT="${DEMO_PROJECT:-tucano-test-demo}"
PORT="${DEMO_PORT:-3199}"
VOLUME="${DEMO_DATA_VOLUME:-${PROJECT}_data}"
ADMIN_USER="${DEMO_ADMIN_USER:-admin}"
ADMIN_PASSWORD="${DEMO_ADMIN_PASSWORD:-demo-admin-password}"
BASE_URL="http://localhost:${PORT}"

ACTION="up"
BUILD=0

usage() {
  sed -n '2,40p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --down) ACTION="down" ;;
    --status) ACTION="status" ;;
    --build) BUILD=1 ;;
    --port) PORT="${2:?--port needs a number}"; shift ;;
    --project) PROJECT="${2:?--project needs a name}"; shift ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "demo: unknown argument $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

for tool in docker node curl python3; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "demo: '$tool' is required but was not found on PATH" >&2
    exit 2
  }
done

# The generated Compose file and everything else the run needs live here, so
# nothing the script writes lands in the working tree.
WORK_DIR="$(mktemp -d)"
COMPOSE_FILE="${WORK_DIR}/docker-compose.demo.yml"
cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

compose() {
  docker compose --project-name "$PROJECT" --file "$COMPOSE_FILE" "$@"
}

# --- the generated stack -----------------------------------------------------

# The API service block below mirrors the committed one, with the auth
# environment spelled out as literal values instead of the `.env` interpolation
# the committed file uses, plus the demo profile and a configurable port and
# data volume. It is generated from a here-document rather than by editing the
# committed YAML so the two cannot silently diverge in the parts that matter:
# the image, the build context, the read-only root and the tmpfs are copied
# verbatim.
write_compose_file() {
  cat >"$COMPOSE_FILE" <<YAML
services:
  api:
    profiles: ["demo"]
    build:
      context: ${REPO_ROOT}
      dockerfile: Dockerfile
    image: tucano-test-api:local
    environment:
      TUCANO_DATA_DIR: /data
      PORT: 3000
      TUCANO_AUTH_REQUIRED: "true"
      TUCANO_JWT_SECRET: ${JWT_SECRET}
      TUCANO_BOOTSTRAP_USERNAME: ${ADMIN_USER}
      TUCANO_BOOTSTRAP_PASSWORD: ${ADMIN_PASSWORD}
    ports:
      - "${PORT}:3000"
    volumes:
      - ${VOLUME}:/data
    read_only: true
    tmpfs:
      - /tmp
    security_opt:
      - no-new-privileges:true
    restart: unless-stopped
    deploy:
      replicas: 1
      resources:
        limits:
          cpus: "1.0"
          memory: 512M

volumes:
  ${VOLUME}:
    name: ${VOLUME}
YAML
}

# The signing secret is generated per run and never written anywhere but the
# temporary Compose file, which the exit trap removes. It is at least 32 bytes
# because the server refuses a shorter one.
JWT_SECRET="$(node -e 'process.stdout.write(require("node:crypto").randomBytes(32).toString("base64url"))')"

# --- waiting ----------------------------------------------------------------

wait_for_health() {
  local attempts=60
  printf 'demo: waiting for %s' "$BASE_URL"
  for _ in $(seq "$attempts"); do
    if curl --silent --fail --connect-timeout 2 --max-time 5 "${BASE_URL}/health" >/dev/null 2>&1; then
      echo " — healthy"
      return 0
    fi
    printf '.'
    sleep 2
  done
  echo
  echo "demo: ${BASE_URL}/health did not answer after $((attempts * 2))s" >&2
  compose logs --tail 50 api >&2 || true
  return 1
}

# --- stages -----------------------------------------------------------------

stage_up() {
  write_compose_file
  local -a build=()
  [ "$BUILD" = "1" ] && build=(--build)
  echo "demo: starting ${PROJECT} on ${BASE_URL}"
  compose --profile demo up -d "${build[@]}"
  wait_for_health
}

stage_seed() {
  echo "demo: seeding the dataset"
  TUCANO_API_URL="$BASE_URL" \
    TUCANO_BOOTSTRAP_USERNAME="$ADMIN_USER" \
    TUCANO_BOOTSTRAP_PASSWORD="$ADMIN_PASSWORD" \
    TUCANO_SEED_AUTH_CMD="docker compose --project-name ${PROJECT} --file ${COMPOSE_FILE} exec -T api tucano-test seed-auth" \
    node "${HERE}/seed.mjs" "$BASE_URL"
}

stage_smoke() {
  echo "demo: running the smoke check"
  SMOKE_USERNAME="$ADMIN_USER" SMOKE_PASSWORD="$ADMIN_PASSWORD" \
    bash "${HERE}/smoke.sh" "$BASE_URL"
}

stage_validate() {
  echo "demo: validating the seeded dataset"
  TUCANO_API_URL="$BASE_URL" \
    TUCANO_BOOTSTRAP_USERNAME="$ADMIN_USER" \
    TUCANO_BOOTSTRAP_PASSWORD="$ADMIN_PASSWORD" \
    node "${HERE}/validate-seed.mjs" "$BASE_URL"
}

stage_matrix() {
  echo "demo: checking the coverage matrix against the contract"
  node "${HERE}/check-matrix.mjs"
}

stage_teardown() {
  echo "demo: tearing the dataset down"
  if ! curl --silent --fail --connect-timeout 2 --max-time 5 "${BASE_URL}/health" >/dev/null 2>&1; then
    echo "demo: the stack is not up, so there is no dataset to tear down"
    return 0
  fi
  TUCANO_API_URL="$BASE_URL" \
    TUCANO_BOOTSTRAP_USERNAME="$ADMIN_USER" \
    TUCANO_BOOTSTRAP_PASSWORD="$ADMIN_PASSWORD" \
    TUCANO_UNSEED_AUTH_CMD="docker compose --project-name ${PROJECT} --file ${COMPOSE_FILE} exec -T api tucano-test unseed-auth" \
    node "${HERE}/teardown.mjs" "$BASE_URL"
}

stage_down() {
  write_compose_file
  stage_teardown
  echo "demo: removing the stack and its volume"
  compose --profile demo down --volumes --timeout 30
}

stage_status() {
  write_compose_file
  compose --profile demo ps || true
  if curl --silent --fail --connect-timeout 2 --max-time 5 "${BASE_URL}/health" >/dev/null 2>&1; then
    echo "demo: ${BASE_URL} is up"
  else
    echo "demo: ${BASE_URL} is not answering"
  fi
}

# --- entry point -------------------------------------------------------------

case "$ACTION" in
  up)
    stage_matrix
    stage_up
    stage_seed
    stage_smoke
    stage_validate
    echo
    echo "demo: PASS — the stack at ${BASE_URL} is seeded, smoke-checked and validated."
    echo "demo: tear it down with scripts/demo.sh --down"
    ;;
  down) stage_down ;;
  status) stage_status ;;
esac
