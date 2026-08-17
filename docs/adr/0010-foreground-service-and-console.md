# Manage aibox through one foreground Service and embedded Console

`aibox serve` starts one foreground aibox Service for one aibox Root and
embeds the Console used for Tenant, Component, Config, Session, image, and
Request Record management. The Service holds an advisory Root-local Service Lock;
`aibox run` stays a direct CLI path and deliberately ignores that lock.

The primary CLI surface is `serve` plus the full `run` command and its verbatim
`--` boundary. Existing management commands remain for one deprecation release
so scripts can migrate, but the Control API is Console-internal and does not
turn the crate into an embedding library.
