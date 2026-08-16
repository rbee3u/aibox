# Keep Traffic global on one trusted listener

The Traffic Proxy is a global host-side service independent of Tenants, Runs,
Sessions, and Coding Agents, and one explicit listener serves both proxy
traffic and the complete Traffic Viewer. Reachable clients are trusted without
authentication or TLS, while outbound routing rejects private upstreams except
for the host-side Fake-IP DNS case, balancing temporary diagnostic access with
a narrow upstream authority boundary.
