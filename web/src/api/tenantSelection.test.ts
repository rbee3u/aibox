import { describe, expect, it } from "vitest";
import { tenantBody, tenantQuery } from "@/api/tenantSelection";

describe("Tenant selection transport encoding", () => {
  it("uses the same encoding for query and wire body", () => {
    const tenant = { kind: "managed", name: "work" } as const;
    expect(tenantQuery(tenant).toString()).toBe("tenant=managed%3Awork");
    expect(tenantBody(tenant)).toEqual({ tenant: "managed:work" });
  });
});
