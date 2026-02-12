#!/usr/bin/env bash
# ssl_check.sh — warn when a TLS certificate is expiring soon.
# Args: <domain> [port]
# Env:  ALERT_DAYS (default 14)

DOMAIN="${1:?Usage: ssl_check.sh <domain> [port]}"
PORT="${2:-443}"
ALERT_DAYS="${ALERT_DAYS:-14}"

expiry=$(echo | openssl s_client -servername "$DOMAIN" -connect "${DOMAIN}:${PORT}" 2>/dev/null \
  | openssl x509 -noout -enddate 2>/dev/null \
  | cut -d= -f2)

if [ -z "$expiry" ]; then
  echo "⚠️ SSL check failed: could not retrieve certificate for ${DOMAIN}:${PORT}"
  exit 0
fi

expiry_epoch=$(date -d "$expiry" +%s 2>/dev/null || date -jf "%b %d %H:%M:%S %Y %Z" "$expiry" +%s 2>/dev/null)
now_epoch=$(date +%s)
days_left=$(( (expiry_epoch - now_epoch) / 86400 ))

if [ "$days_left" -le "$ALERT_DAYS" ]; then
  echo "🔐 SSL certificate expiring soon!"
  echo "Domain: $DOMAIN"
  echo "Expires: $expiry"
  echo "Days left: $days_left"
else
  echo "NOUPDATE"
fi
