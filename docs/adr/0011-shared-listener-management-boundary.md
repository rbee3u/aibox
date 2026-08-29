# Separate loopback management from the Request Proxy on one listener

The AIBox Service uses one explicit listener for two trust domains. Every
Console data, body, and event endpoint belongs to the Control API under
`/_aibox/api/`; `/` and
every `/_aibox/*` path are reserved for management and require an actual
loopback TCP peer plus a loopback `Host`; unknown reserved paths fail locally
and never enter proxy routing. All other paths are Request Proxy input and may
arrive through a wildcard listener used by containers.

Management mutations additionally require JSON, a same-origin `Origin`, and a
random startup CSRF token obtained from the bootstrap endpoint. The Console is
served with a restrictive Content Security Policy. This boundary adds no auth,
TLS, or admission policy to the Request Proxy itself.
