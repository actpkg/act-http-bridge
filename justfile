wasm := "target/wasm32-wasip2/release/act_http_bridge.wasm"

act := env("ACT", "npx @actcore/act")
actbuild := env("ACT_BUILD", "npx @actcore/act-build")
hurl := env("HURL", "hurl")
registry := env("OCI_REGISTRY", "actpkg.dev/library")

# Bridge (under test)
port := `shuf -i 10000-29999 -n 1`
addr := "[::1]:" + port
baseurl := "http://" + addr

# Upstream ACT-HTTP server — a real component served by `act run --http`,
# which the bridge proxies to. Using `time` (simple, pure compute, one tool).
upstream_image := "actpkg.dev/library/time:latest"
upstream_port := `shuf -i 10000-29999 -n 1`
upstream_addr := "[::1]:" + upstream_port
upstream_url := "http://" + upstream_addr

# Fetch WIT deps from the registry (ghcr.io/actcore) into wit/deps/.
# wkg-registry.toml maps the act namespace -> actcore.dev (well-known -> ghcr.io/actcore).
init:
    WKG_CONFIG_FILE=wkg-registry.toml wkg wit fetch --type wit

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
    {{act}} run {{wasm}} --http --listen "{{addr}}" --allow wasi:http &
    PIDS+=($!)
    curl --retry 60 --retry-connrefused --retry-delay 1 -fsS -o /dev/null {{baseurl}}/info
    curl --retry 60 --retry-connrefused --retry-delay 1 -fsS -o /dev/null "{{upstream_url}}/info"
    {{hurl}} --test \
      --variable "baseurl={{baseurl}}" \
      --variable "upstream_url={{upstream_url}}" \
      e2e/*.hurl

publish:
    #!/usr/bin/env bash
    set -euo pipefail
    INFO=$({{act}} inspect component-manifest {{wasm}})
    NAME=$(echo "$INFO" | jq -r .std.name)
    VERSION=$(echo "$INFO" | jq -r .std.version)
    OUTPUT=$({{actbuild}} push {{wasm}} "{{registry}}/$NAME:$VERSION" \
      --skip-if-exists \
      --also-tag latest 2>&1) || { echo "$OUTPUT" >&2; exit 1; }
    echo "$OUTPUT"
    DIGEST=$(echo "$OUTPUT" | grep "^Digest:" | awk '{print $2}' || true)
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
      echo "image={{registry}}/$NAME" >> "$GITHUB_OUTPUT"
      echo "digest=$DIGEST" >> "$GITHUB_OUTPUT"
    fi
