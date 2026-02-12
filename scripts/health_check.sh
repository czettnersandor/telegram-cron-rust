#!/usr/bin/env bash
# health_check.sh — check if a service/URL is reachable
# Returns NOUPDATE if all good, or a message to send via Telegram.

TARGETS=(
  "https://example.com"
  # "http://localhost:8080/health"
)

FAILED=()

for url in "${TARGETS[@]}"; do
  # -s silent, -o discard body, -w write HTTP code, --max-time 10s
  code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 10 "$url" 2>/dev/null)
  if [[ "$code" != "200" ]]; then
    FAILED+=("$url (HTTP $code)")
  fi
done

if [ ${#FAILED[@]} -eq 0 ]; then
  echo "NOUPDATE"
else
  echo "🚨 Health check failed:"
  for item in "${FAILED[@]}"; do
    echo "  • $item"
  done
fi
