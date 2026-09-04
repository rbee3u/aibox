import { describe, expect, it } from "vitest";

import { abbreviateTenantHome } from "@/shared/lib/hostHome";

describe("host home abbreviation", () => {
  it("shortens a path inside the Host Home", () => {
    expect(abbreviateTenantHome("/home/test/.aibox/tenants/work", "/home/test")).toBe(
      "~/.aibox/tenants/work",
    );
    expect(abbreviateTenantHome("/home/test", "/home/test")).toBe("~");
    expect(abbreviateTenantHome("/var/lib/aibox", "/home/test")).toBe("/var/lib/aibox");
    expect(abbreviateTenantHome("/var/lib/aibox", null)).toBe("/var/lib/aibox");
  });
});
