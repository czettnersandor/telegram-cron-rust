#!/usr/bin/env python3
"""
Example script: Backup status checker
Checks if backup files exist and are recent
"""

import os
import sys
from datetime import datetime, timedelta
from pathlib import Path

# Configuration
MAX_AGE_HOURS = 26  # Alert if backup is older than this

def check_backup_status(backup_dir):
    """Check if recent backups exist"""

    # For demo purposes, just create a successful backup message
    # In real usage, you would check actual backup files

    if not os.path.exists(backup_dir):
        print(f"⚠️ Backup Directory Missing!")
        print(f"Expected location: {backup_dir}")
        return

    # Find most recent backup file (example)
    backup_files = list(Path(backup_dir).glob("*.sql.gz"))

    if not backup_files:
        print("❌ No Backup Files Found!")
        print(f"Directory: {backup_dir}")
        return

    # Get most recent backup
    latest_backup = max(backup_files, key=lambda p: p.stat().st_mtime)
    backup_time = datetime.fromtimestamp(latest_backup.stat().st_mtime)
    age = datetime.now() - backup_time

    # Check if backup is too old
    if age > timedelta(hours=MAX_AGE_HOURS):
        print(f"⚠️ Backup is Outdated!")
        print(f"")
        print(f"Latest backup: {latest_backup.name}")
        print(f"Age: {age.days} days, {age.seconds // 3600} hours")
        print(f"Threshold: {MAX_AGE_HOURS} hours")
    else:
        # Backup is recent - send success notification
        print(f"✅ Backup Status OK")
        print(f"")
        print(f"Latest: {latest_backup.name}")
        print(f"Size: {latest_backup.stat().st_size / (1024**2):.1f} MB")
        print(f"Age: {age.seconds // 3600}h {(age.seconds % 3600) // 60}m")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Error: No backup directory provided", file=sys.stderr)
        sys.exit(1)

    backup_dir = sys.argv[1]
    check_backup_status(backup_dir)
