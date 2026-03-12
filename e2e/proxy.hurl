# List tools through the bridge (proxying the time component)
POST {{host}}/tools
{"config": {"url": "{{backend}}"}}
HTTP 200
[Asserts]
jsonpath "$.tools" count > 0
jsonpath "$.tools[0].name" == "get-current-time"

# Call get-current-time through the bridge
POST {{host}}/tools/get-current-time
{"arguments": {}, "config": {"url": "{{backend}}"}}
HTTP 200
[Asserts]
jsonpath "$.content[0].mime_type" == "text/plain"
