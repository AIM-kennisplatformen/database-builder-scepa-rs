#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
env_file="${script_dir}/../.env.ingest"

if [[ -f "${env_file}" ]]; then
  # shellcheck source=../.env.ingest
  source "${env_file}"
fi

ssh_target="${SCEPA_SSH_TARGET:-}"
open_browser=true

usage() {
  printf 'Usage: %s [--no-open] [user@host]\n' "$(basename "$0")"
  printf '\nOpens one SSH session containing every deployed service tunnel.\n'
}

while (($#)); do
  case "$1" in
    --no-open)
      open_browser=false
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -* )
      printf 'Unknown option: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
    *)
      ssh_target="$1"
      ;;
  esac
  shift
done

if [[ -z "${ssh_target}" ]]; then
  printf 'SSH target is required. Set SCEPA_SSH_TARGET in %s or pass user@host.\n' "${env_file}" >&2
  usage >&2
  exit 2
fi

forward_args=(
  -L 13000:127.0.0.1:3000   # Upload API
  -L 18002:127.0.0.1:8002   # Literature MCP
  -L 11729:127.0.0.1:1729   # TypeDB gRPC
  -L 18000:127.0.0.1:8000   # TypeDB HTTP
  -L 15432:127.0.0.1:5432   # Ingestion PostgreSQL
  -L 16333:127.0.0.1:6333   # Qdrant HTTP/dashboard
  -L 16334:127.0.0.1:6334   # Qdrant gRPC
  -L 13900:127.0.0.1:3900   # Garage S3 API
  -L 13903:127.0.0.1:3903   # Garage admin API
  -L 18070:127.0.0.1:8070   # GROBID
  -L 18080:127.0.0.1:8080   # Restate ingress
  -L 19070:127.0.0.1:9070   # Restate admin UI/API
  -L 15122:127.0.0.1:5122   # Restate fabric
  -L 10092:127.0.0.1:10092  # Studio backend
  -L 16379:127.0.0.1:6379   # Studio Redis
  -L 15433:127.0.0.1:5433   # Studio PostgreSQL
)

browser_urls=(
  "${QDRANT_DASHBOARD_URL:-http://127.0.0.1:16333/dashboard}"
  "${RESTATE_ADMIN_URL:-http://127.0.0.1:19070}"
  "http://127.0.0.1:18070"
)

printf 'Opening deployed-service tunnels through %s...\n' "${ssh_target}"
ssh \
  -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=30 \
  -o ServerAliveCountMax=3 \
  "${forward_args[@]}" \
  -N "${ssh_target}" &
ssh_pid=$!

cleanup() {
  if kill -0 "${ssh_pid}" 2>/dev/null; then
    kill "${ssh_pid}" 2>/dev/null || true
    wait "${ssh_pid}" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

sleep 1
if ! kill -0 "${ssh_pid}" 2>/dev/null; then
  wait "${ssh_pid}"
fi

printf '\nBrowser interfaces:\n'
printf '  Qdrant dashboard:      %s\n' "${browser_urls[0]}"
printf '  Restate admin UI:      %s\n' "${browser_urls[1]}"
printf '  GROBID:                %s\n' "${browser_urls[2]}"

printf '\nAPIs and native clients:\n'
printf '  Upload API:            http://127.0.0.1:13000\n'
printf '  Literature MCP:        http://127.0.0.1:18002/mcp\n'
printf '  TypeDB gRPC:           127.0.0.1:11729\n'
printf '  TypeDB HTTP:           http://127.0.0.1:18000\n'
printf '  Ingestion PostgreSQL:  127.0.0.1:15432\n'
printf '  Qdrant gRPC:           127.0.0.1:16334\n'
printf '  Garage S3 API:         http://127.0.0.1:13900\n'
printf '  Garage admin API:      http://127.0.0.1:13903\n'
printf '  Restate ingress:       http://127.0.0.1:18080\n'
printf '  Restate fabric:        127.0.0.1:15122\n'
printf '  Studio backend:        http://127.0.0.1:10092\n'
printf '  Studio Redis:          127.0.0.1:16379\n'
printf '  Studio PostgreSQL:     127.0.0.1:15433\n'

if [[ "${open_browser}" == true ]]; then
  opener=""
  if command -v open >/dev/null 2>&1; then
    opener="open"
  elif command -v xdg-open >/dev/null 2>&1; then
    opener="xdg-open"
  fi

  if [[ -n "${opener}" ]]; then
    for url in "${browser_urls[@]}"; do
      "${opener}" "${url}" >/dev/null 2>&1 || true
    done
  else
    printf '\nNo supported browser opener found; use the URLs above.\n'
  fi
fi

printf '\nTunnels are active. Press Ctrl-C to close them all.\n'
wait "${ssh_pid}"
