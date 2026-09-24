/**
 * Wire types shared by more than one Control API domain module.
 *
 * The generated bindings are Rust-owned and named after their Rust types, so
 * this is where a handful of them take the name the Console uses. Every layer
 * outside `api/` imports these from here: eslint keeps features and
 * `features/common` out of `api/generated/*` so a rename on the Rust side has
 * one place to land.
 */

import type { BootstrapResponse, TenantRow } from "@/api/generated/wire";

export type Bootstrap = BootstrapResponse;
export type { TenantRow };
