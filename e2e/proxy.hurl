# List tools through the bridge (proxying the time component)
POST {{baseurl}}/tools
{"metadata": {"url": "{{backend}}"}}
HTTP 200
[Asserts]
jsonpath "$.tools" count > 0
jsonpath "$.tools[0].name" == "get-current-time"

# Call get-current-time through the bridge
POST {{baseurl}}/tools/get-current-time
{"arguments": {}, "metadata": {"url": "{{backend}}"}}
HTTP 200
[Asserts]
jsonpath "$.content[0].mime_type" == "text/plain"
