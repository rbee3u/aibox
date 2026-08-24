# Use one fixed Runtime Image

aibox uses the local image tag `aibox:latest` for `aibox run`, Console Overview
builds, and Managed Tenant Component installation. Image overrides and the
associated validation surface were removed so every execution path shares one
inspectable image contract and users cannot accidentally build one tag while
running another.

The trade-off is that selecting a custom local image is no longer supported;
development changes should be made in the embedded Dockerfile and rebuilt with
**Build without cache** in Console Overview.
