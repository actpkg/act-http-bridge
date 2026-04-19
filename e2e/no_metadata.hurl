# Bridge requires `url` in metadata — without it, returns invalid-args error.
POST {{baseurl}}/tools
{}
HTTP *
[Asserts]
jsonpath "$.error.kind" == "std:invalid-args"
jsonpath "$.error.message" contains "url"
