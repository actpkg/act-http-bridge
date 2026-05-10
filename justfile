wasm := "target/wasm32-wasip2/release/act_http_bridge.wasm"

act := env("ACT", "npx @actcore/act")
actbuild := env("ACT_BUILD", "npx @actcore/act-build")
hurl := env("HURL", "npx @orangeopensource/hurl")
registry := env("OCI_REGISTRY", "ghcr.io/actpkg")

# Bridge (under test)
port := `npx get-port-cli`
addr := "[::1]:" + port
baseurl := "http://" + addr

# Upstream ACT-HTTP server — a real component served by `act run --http`,
# which the bridge proxies to. Using `time` (simple, pure compute, one tool).
upstream_image := "ghcr.io/actpkg/time:0.2.0"
upstream_port := `npx get-port-cli`
upstream_addr := "[::1]:" + upstream_port
upstream_url := "http://" + upstream_addr

init:
    wit-deps

setup: init
    prek install

build:
    cargo build --release
    {{actbuild}} pack {{wasm}}

test:
    #!/usr/bin/env bash
    set -euo pipefail
    PIDS=()
    trap 'kill "${PIDS[@]}" 2>/dev/null' EXIT
    {{act}} run {{upstream_image}} --http --listen "{{upstream_addr}}" &
    PIDS+=($!)
    {{act}} run {{wasm}} --http --listen "{{addr}}" --http-policy open &
    PIDS+=($!)
    npx wait-on -t 180s {{baseurl}}/info "{{upstream_url}}/info"
    {{hurl}} --test \
      --variable "baseurl={{baseurl}}" \
      --variable "upstream_url={{upstream_url}}" \
      e2e/*.hurl

publish:
    #!/usr/bin/env bash
    set -euo pipefail
    INFO=$({{act}} info {{wasm}} --format json)
    NAME=$(echo "$INFO" | jq -r .name)
    VERSION=$(echo "$INFO" | jq -r .version)
    SOURCE=$(git remote get-url origin 2>/dev/null | sed 's/\.git$//' | sed 's|git@github.com:|https://github.com/|' || echo "")
    OUTPUT=$({{actbuild}} push {{wasm}} "{{registry}}/$NAME:$VERSION" \
      --skip-if-exists \
      --also-tag latest \
      --source "$SOURCE" 2>&1) || { echo "$OUTPUT" >&2; exit 1; }
    echo "$OUTPUT"
    DIGEST=$(echo "$OUTPUT" | grep "^Digest:" | awk '{print $2}' || true)
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
      echo "image={{registry}}/$NAME" >> "$GITHUB_OUTPUT"
      echo "digest=$DIGEST" >> "$GITHUB_OUTPUT"
    fi
