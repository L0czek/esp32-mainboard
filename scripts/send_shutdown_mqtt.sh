#!/usr/bin/env bash
set -euo pipefail

if ! command -v mosquitto_pub >/dev/null 2>&1; then
  echo "error: mosquitto_pub not found in PATH" >&2
  exit 1
fi

HOST="${MQTT_HOST:-${1:-localhost}}"

PORT="${MQTT_PORT:-1883}"
TOPIC="${MQTT_TOPIC:-cmd/shutdown}"
PAYLOAD="${MQTT_PAYLOAD:-SHUTDOWN}"

cmd=(mosquitto_pub -h "$HOST" -p "$PORT" -t "$TOPIC" -m "$PAYLOAD")

if [[ -n "${MQTT_USER:-}" ]]; then
  cmd+=( -u "$MQTT_USER" )
fi

if [[ -n "${MQTT_PASSWORD:-}" ]]; then
  cmd+=( -P "$MQTT_PASSWORD" )
fi

"${cmd[@]}"

echo "published '$PAYLOAD' to '$TOPIC' on $HOST:$PORT"
