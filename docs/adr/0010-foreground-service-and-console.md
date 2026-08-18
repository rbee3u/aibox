# Manage aibox through one foreground Service and embedded Console

`aibox serve` starts one foreground aibox Service for one aibox Root and
embeds the Console used for Tenant, Component, Config, Session, image, and
Request Record management. The Service holds an advisory Root-local Service Lock;
`aibox run` stays a direct CLI path and deliberately ignores that lock.

The public CLI surface is `serve`, `run`, and `build`; `run` retains its
verbatim `--` boundary. Tenant, Component, Config, and Session management is
Console-only. The Control API is Console-internal and does not turn the crate
into an embedding library.
