# Keep the Request Proxy global on one trusted listener

The Request Proxy is global and independent of Tenants, Runs, Sessions, and
Coding Agents. ADR 0010 makes it an always-on part of the foreground aibox
Service, while ADR 0011 reserves loopback management routes on the same
explicit listener. Reachable Request Proxy clients remain trusted without
authentication or TLS, and outbound routing still rejects private upstreams
except for the host-side Fake-IP DNS case.
