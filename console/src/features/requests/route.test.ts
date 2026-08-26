import { describe, expect, it } from "vitest";
import { readRequestsRoute, requestsSearch } from "@/features/requests/route";

describe("Requests route codec", () => {
  it("defaults an empty search to the first page without a selection", () => {
    expect(readRequestsRoute("")).toEqual({ page: 1, request: null, tab: "summary" });
  });

  it("reads a complete selection", () => {
    expect(readRequestsRoute("?page=3&request=abc&tab=response")).toEqual({
      page: 3,
      request: "abc",
      tab: "response",
    });
  });

  it("replaces malformed, zero, negative, and unsafe page numbers with page 1", () => {
    for (const search of [
      "?page=0",
      "?page=-2",
      "?page=x",
      "?page=1.5",
      "?page=99999999999999999999",
    ]) {
      expect(readRequestsRoute(search).page, search).toBe(1);
    }
  });

  it("ignores a Tab without a selected Request and an unknown Tab name", () => {
    expect(readRequestsRoute("?tab=response").tab).toBe("summary");
    expect(readRequestsRoute("?request=abc&tab=nowhere").tab).toBe("summary");
  });

  it("treats a blank Request id as absent", () => {
    expect(readRequestsRoute("?request=%20%20").request).toBeNull();
  });

  it("omits default values when serializing", () => {
    expect(requestsSearch({ page: 1, request: null, tab: "summary" })).toBe("");
    expect(requestsSearch({ page: 1, request: "abc", tab: "summary" })).toBe("?request=abc");
    expect(requestsSearch({ page: 2, request: "abc", tab: "request" })).toBe(
      "?page=2&request=abc&tab=request",
    );
  });

  it("drops a Tab that has no selected Request", () => {
    expect(requestsSearch({ page: 1, request: null, tab: "response" })).toBe("");
  });

  it("round-trips every canonical search", () => {
    for (const search of ["", "?request=abc", "?page=4", "?page=4&request=abc&tab=response"]) {
      expect(requestsSearch(readRequestsRoute(search)), search).toBe(search);
    }
  });
});
