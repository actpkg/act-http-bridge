# Call an upstream tool through the bridge. `get_current_time` returns RFC3339
# (UTC by default; with a timezone it returns that TZ).
POST {{baseurl}}/tools/get_current_time
{"arguments": {}, "metadata": {"url": "{{upstream_url}}"}}
HTTP 200
[Asserts]
jsonpath "$.content[0].data" matches "\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}"

# With a specific timezone
POST {{baseurl}}/tools/get_current_time
{"arguments": {"timezone": "Europe/Moscow"}, "metadata": {"url": "{{upstream_url}}"}}
HTTP 200
[Asserts]
jsonpath "$.content[0].data" matches "\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}"

# Unknown tool on the upstream — bridge should surface the error
POST {{baseurl}}/tools/nonexistent_tool_xyz
{"arguments": {}, "metadata": {"url": "{{upstream_url}}"}}
HTTP *
[Asserts]
jsonpath "$.error" exists
