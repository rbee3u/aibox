export type ModuleId = "overview" | "tenants" | "configs" | "sessions" | "requests";

/**
 * Writes a query for the module that owns it. The active module is implied by
 * the page holding the callback, so a page never names itself.
 */
export type ModuleLocationChange = (query: URLSearchParams, replace?: boolean) => void;

/** Moves to another module, optionally with its initial query. */
export type ConsoleNavigate = (module: ModuleId, query?: URLSearchParams) => void;

export function currentPageSearch(): URLSearchParams {
  return new URLSearchParams(window.location.search);
}

export const CONSOLE_BASE_PATH = "/_aibox/ui";

export function modulePath(module: ModuleId, query?: URLSearchParams): string {
  const suffix = query?.toString();
  return `${CONSOLE_BASE_PATH}/${module}${suffix ? `?${suffix}` : ""}`;
}
