#!/bin/bash
# Launch Neovim with your real config + this plugin loaded.
# Usage: ./scripts/nvim-test.sh [file]
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec nvim -u "$SCRIPT_DIR/test-init.lua" "$@"
