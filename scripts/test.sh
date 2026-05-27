#!/bin/bash
# Usage: ./scripts/test.sh [image_path]
# Must be run from a real terminal (writes Kitty protocol to /dev/tty).
set -e

BINARY="./bin/nvim-gfx"
IMAGE="${1:-./53dd0a9a-5dad-4f78-a794-b630c22750b3.png}"

if [ ! -f "$BINARY" ]; then
    echo "Binary not found. Run: cargo build --release && cp target/release/nvim-gfx bin/nvim-gfx"
    exit 1
fi

if [ ! -f "$IMAGE" ]; then
    echo "Test image not found: $IMAGE"
    exit 1
fi

echo "Test 1: image open emits ready"
RESULT=$(printf '{"cmd":"show","path":"%s","row":0,"col":0,"width":80,"height":24}\n{"cmd":"quit"}\n' "$IMAGE" | "$BINARY" 2>/dev/null)
if echo "$RESULT" | grep -q '"event":"ready"'; then
    echo "  PASS"
else
    echo "  FAIL: $RESULT"
    exit 1
fi

echo "Test 2: bad path emits error"
RESULT=$(printf '{"cmd":"show","path":"/nonexistent.png","row":0,"col":0,"width":80,"height":24}\n' | timeout 3 "$BINARY" 2>/dev/null || true)
if echo "$RESULT" | grep -q '"event":"error"'; then
    echo "  PASS"
else
    echo "  FAIL: $RESULT"
    exit 1
fi

echo "All tests passed."
