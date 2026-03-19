wasm := "target/wasm32-wasip2/release/act_http_bridge.wasm"
act := env("ACT", "act")
port := `python3 -c 'import socket; s=socket.socket(socket.AF_INET, socket.SOCK_STREAM); s.bind(("", 0)); print(s.getsockname()[1]); s.close()'`
addr := "[::1]:" + port
baseurl := "http://" + addr

init:
    wit-deps

setup: init
    prek install

build:
    cargo build --target wasm32-wasip2 --release

test:
    #!/usr/bin/env bash
    BACKEND_PORT=$(python3 -c 'import socket; s=socket.socket(socket.AF_INET, socket.SOCK_STREAM); s.bind(("", 0)); print(s.getsockname()[1]); s.close()')
    BACKEND="http://[::1]:$BACKEND_PORT"
    # Start time component as backend
    {{act}} serve ../../components/time/target/wasm32-wasip2/release/component_time.wasm --listen "[::1]:$BACKEND_PORT" &
    BACKEND_PID=$!
    # Start bridge
    {{act}} serve {{wasm}} --listen "{{addr}}" &
    BRIDGE_PID=$!
    trap "kill $BRIDGE_PID $BACKEND_PID 2>/dev/null" EXIT
    npx wait-on $BACKEND/info
    npx wait-on {{baseurl}}/info
    hurl --test --variable "baseurl={{baseurl}}" --variable "backend=$BACKEND" e2e/*.hurl
