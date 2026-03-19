# act-http-bridge requires metadata — without it, returns error
POST {{baseurl}}/tools
{}
HTTP *
[Asserts]
jsonpath "$.error" exists
