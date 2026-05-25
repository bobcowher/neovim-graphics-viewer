#!/usr/bin/env bash
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec nvim -u "$DIR/scripts/test-init.lua" "$@"
