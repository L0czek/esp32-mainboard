#!/usr/bin/env bash
set -euo pipefail

if ! command -v mosquitto_pub >/dev/null 2>&1; then
  echo "error: mosquitto_pub not found in PATH" >&2
  exit 1
fi

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "${SCRIPT_DIR}/.." && pwd)

DEFAULT_ELF="${REPO_ROOT}/target/riscv32imac-unknown-none-elf/release/test_stand_controller"
ELF_PATH="${TEST_STAND_ELF:-${1:-$DEFAULT_ELF}}"
HOST="${MQTT_HOST:-${2:-localhost}}"
PORT="${MQTT_PORT:-1883}"
TOPIC="${MQTT_TOPIC:-shared/firmware/test_stand_controller/elf}"

if [[ ! -f "${ELF_PATH}" ]]; then
  echo "error: ELF not found at ${ELF_PATH}" >&2
  echo "hint: build test_stand_controller first or set TEST_STAND_ELF=/path/to/elf" >&2
  exit 1
fi

cmd=(mosquitto_pub -h "$HOST" -p "$PORT" -t "$TOPIC" -r -f "$ELF_PATH")

if [[ -n "${MQTT_USER:-}" ]]; then
  cmd+=( -u "$MQTT_USER" )
fi

if [[ -n "${MQTT_PASSWORD:-}" ]]; then
  cmd+=( -P "$MQTT_PASSWORD" )
fi

"${cmd[@]}"

echo "published ${ELF_PATH} to retained topic '${TOPIC}' on ${HOST}:${PORT}"
