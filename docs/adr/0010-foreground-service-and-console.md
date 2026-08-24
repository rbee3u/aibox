# Manage aibox through one foreground Service and embedded Console

`aibox console` starts one foreground aibox Service for one aibox Root and
embeds the Console used for Tenant, Component, Config, Session, image, and
Request management. The Service holds an advisory Root-local Service Lock;
`aibox run` stays a direct CLI path and deliberately ignores that lock.

The public CLI surface is `console` and `run`; `run` retains its verbatim `--`
boundary. Runtime Image, Tenant, Component, Config, and Session management is
Console-only. The Control API is Console-internal and does not turn the crate
into an embedding library.
