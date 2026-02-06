#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE_DEFAULT="$ROOT_DIR/docker-compose.yml"
COMPOSE_FILE="${COMPOSE_FILE:-$COMPOSE_FILE_DEFAULT}"

pick_compose() {
  if docker compose version >/dev/null 2>&1; then
    echo "docker compose"
    return
  fi
  if command -v docker-compose >/dev/null 2>&1; then
    echo "docker-compose"
    return
  fi
  echo "docker compose not found" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Usage:
  scripts/compose.sh build [compose args...]
  scripts/compose.sh up    [compose args...]
  scripts/compose.sh down  [compose args...]
  scripts/compose.sh logs  [compose args...]
  scripts/compose.sh ps    [compose args...]

Notes:
  - Uses docker compose if available, falls back to docker-compose.
  - Set COMPOSE_FILE to override the compose file path.
  - Set HORIZONS_REF to control the Horizons git ref used during docker build.

Examples:
  HORIZONS_REF=main scripts/compose.sh build
  scripts/compose.sh up
  scripts/compose.sh logs opencraw
EOF
}

cmd="${1:-}"
if [[ -z "$cmd" || "$cmd" == "-h" || "$cmd" == "--help" ]]; then
  usage
  exit 0
fi
shift || true

COMPOSE_CMD="$(pick_compose)"

cd "$ROOT_DIR"

case "$cmd" in
  build)
    $COMPOSE_CMD -f "$COMPOSE_FILE" build "$@"
    ;;
  up)
    $COMPOSE_CMD -f "$COMPOSE_FILE" up -d --build --remove-orphans "$@"
    ;;
  down)
    $COMPOSE_CMD -f "$COMPOSE_FILE" down "$@"
    ;;
  logs)
    $COMPOSE_CMD -f "$COMPOSE_FILE" logs -f "$@"
    ;;
  ps)
    $COMPOSE_CMD -f "$COMPOSE_FILE" ps "$@"
    ;;
  *)
    echo "Unknown command: $cmd" >&2
    usage
    exit 2
    ;;
esac

