#!/bin/sh

set -e

echo "Copying telegram-cron to NAS..."
ssh nas "source \"\$HOME/.cargo/env\" && cd telegram-cron-rust && git pull && cargo install --path ."
