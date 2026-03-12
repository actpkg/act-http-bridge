#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
COMPONENT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ACT="${ACT:-act}"
WASM="$COMPONENT_DIR/target/wasm32-wasip2/release/act_http_bridge.wasm"

# We also need a "backend" ACT component to proxy.
# Use the time component as a simple backend.
BACKEND_WASM="$COMPONENT_DIR/../time/target/wasm32-wasip2/release/component_time.wasm"

if [ ! -f "$WASM" ]; then
  echo "WASM not found: $WASM"
  echo "Run: cargo build --release --target wasm32-wasip2"
  exit 1
fi

if [ ! -f "$BACKEND_WASM" ]; then
  echo "Backend WASM not found: $BACKEND_WASM"
  echo "Build the time component first"
  exit 1
fi

# Find two free ports
BACKEND_PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("",0)); print(s.getsockname()[1]); s.close()')
BRIDGE_PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("",0)); print(s.getsockname()[1]); s.close()')

cleanup() {
  [ -n "${BACKEND_PID:-}" ] && kill "$BACKEND_PID" 2>/dev/null || true
  [ -n "${BRIDGE_PID:-}" ] && kill "$BRIDGE_PID" 2>/dev/null || true
}
trap cleanup EXIT

# Start the backend (time component)
"$ACT" serve "$BACKEND_WASM" --listen "[::1]:$BACKEND_PORT" &
BACKEND_PID=$!

# Wait for backend to be ready
for i in $(seq 1 30); do
  if curl -sf "http://[::1]:$BACKEND_PORT/info" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

# Start the bridge component
"$ACT" serve "$WASM" --listen "[::1]:$BRIDGE_PORT" &
BRIDGE_PID=$!

# Wait for bridge to be ready
for i in $(seq 1 30); do
  if curl -sf "http://[::1]:$BRIDGE_PORT/info" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

# Run tests — pass both host URLs as variables
hurl --test --variable "host=http://[::1]:$BRIDGE_PORT" \
            --variable "backend=http://[::1]:$BACKEND_PORT" \
            "$SCRIPT_DIR"/*.hurl
