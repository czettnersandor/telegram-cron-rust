#!/usr/bin/env bash
# disk_usage.sh — alert when disk usage exceeds a threshold.
# Args: <mount_point> <threshold_percent>
# Returns NOUPDATE if usage is below threshold.

MOUNT="${1:-/}"
THRESHOLD="${2:-85}"

usage=$(df -h "$MOUNT" | awk 'NR==2 {gsub(/%/,"",$5); print $5}')
used=$(df -h "$MOUNT" | awk 'NR==2 {print $3}')
avail=$(df -h "$MOUNT" | awk 'NR==2 {print $4}')
total=$(df -h "$MOUNT" | awk 'NR==2 {print $2}')

if [ -z "$usage" ]; then
  echo "NOUPDATE"
  exit 0
fi

if [ "$usage" -ge "$THRESHOLD" ]; then
  echo "💾 Disk usage alert on $(hostname)"
  echo "Mount: $MOUNT"
  echo "Usage: ${usage}% (${used} used / ${total} total, ${avail} free)"
else
  echo "NOUPDATE"
fi
