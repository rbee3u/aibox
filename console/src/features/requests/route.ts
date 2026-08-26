import { readEnum, readPositiveInteger, readTrimmed, searchString } from "@/shared/lib/queryParams";
import type { DetailTab } from "@/features/requests/viewTypes";

export const DETAIL_TABS: readonly DetailTab[] = ["summary", "request", "response"];

export interface RequestsRoute {
  page: number;
  request: string | null;
  tab: DetailTab;
}

export function readRequestsRoute(search: string): RequestsRoute {
  const params = new URLSearchParams(search);
  const request = readTrimmed(params, "request");
  return {
    page: readPositiveInteger(params, "page", 1),
    request,
    tab: request ? readEnum(params, "tab", DETAIL_TABS, "summary") : "summary",
  };
}

export function requestsSearch(value: RequestsRoute): string {
  const params = new URLSearchParams();
  if (value.page > 1) params.set("page", String(value.page));
  if (value.request) params.set("request", value.request);
  if (value.request && value.tab !== "summary") params.set("tab", value.tab);
  return searchString(params);
}
