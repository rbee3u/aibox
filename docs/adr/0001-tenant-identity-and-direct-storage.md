# Model Tenant identity through direct storage

AIBox defines Managed Tenants by real directories under a dedicated, unmarked
AIBox Root. Host Tenant native state stays in the Host Home; its Named Config
catalog stays under the Root. Direct storage avoids a registry but requires
structural validation of untrusted filesystem state.
