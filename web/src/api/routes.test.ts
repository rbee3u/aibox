import { describe, expect, it } from "vitest";
import { routes } from "@/api/generated/routes";
import { controlRoute } from "@/test/controlRoutes";

describe("Rust-owned Control route manifest", () => {
  it("contains the stable method and path for every Console endpoint", () => {
    const entries = Object.entries(routes);
    expect(entries.length).toBeGreaterThan(30);
    expect(new Set(entries.map(([key]) => key)).size).toBe(entries.length);
    expect(entries.every(([, route]) => route.method === "GET" || route.method === "POST")).toBe(
      true,
    );
    expect(routes.components_list).toEqual({
      method: "GET",
      path: "/_aibox/api/components",
    });
    expect(routes.request_detail.path).toContain("{id}");
    expect(controlRoute("request_detail", { id: "request/id" })).toBe(
      "/_aibox/api/requests/request%2Fid",
    );
  });
});
