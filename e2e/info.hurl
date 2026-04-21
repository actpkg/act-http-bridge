GET {{baseurl}}/info
HTTP 200
[Asserts]
jsonpath "$.std.name" == "act_http_bridge"
jsonpath "$.std.version" isString
jsonpath "$.std.capabilities['wasi:http']" exists
