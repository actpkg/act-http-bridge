# Bridge should proxy list-tools to the upstream ACT-HTTP server
# and surface its tools (time component exposes `get-current-time`).
POST {{baseurl}}/tools
{"metadata": {"url": "{{upstream_url}}"}}
HTTP 200
[Asserts]
jsonpath "$.tools" count >= 1
jsonpath "$.tools[*].name" contains "get-current-time"
