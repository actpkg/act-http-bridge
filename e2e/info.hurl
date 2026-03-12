# Component info
GET {{host}}/info
HTTP 200
[Asserts]
jsonpath "$.name" == "act-http-bridge"
jsonpath "$.version" == "0.1.0"
