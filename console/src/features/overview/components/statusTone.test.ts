import { describe, expect, it } from "vitest";

import { dockerTone, imageTone } from "@/features/overview/components/statusTone";

describe("statusTone", () => {
  it("reports a healthy fact as good rather than neutral", () => {
    expect(dockerTone("available")).toBe("good");
    expect(imageTone("built")).toBe("good");
  });

  it("separates an unusable fact from one that cannot be reported", () => {
    expect(dockerTone("unavailable")).toBe("error");
    expect(imageTone("missing")).toBe("warning");
  });

  it("keeps neutral for a fact the Service has not reported", () => {
    expect(dockerTone()).toBe("neutral");
    expect(imageTone("unknown")).toBe("neutral");
    expect(imageTone()).toBe("neutral");
  });
});
