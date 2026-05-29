#!/bin/bash
# Headless Neovim runner for the Lua test harness.
# Usage: ./scripts/lua-tests.sh
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec nvim --headless --clean -u NORC -c "luafile $SCRIPT_DIR/lua-tests.lua"
