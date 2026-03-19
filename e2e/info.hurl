GET {{baseurl}}/info
HTTP 200
[Asserts]
jsonpath "$['std:name']" == "act-http-bridge"
jsonpath "$['std:version']" isString
