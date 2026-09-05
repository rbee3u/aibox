import { describe, expect, it } from "vitest";

import {
  buildActionTone,
  cachelessBuildInline,
  imageTitle,
  imageTone,
  shortImageId,
} from "@/features/overview/components/runtimeImage";

describe("runtimeImage", () => {
  it("treats a Built image as healthy and a missing image as warning", () => {
    expect(imageTone("built")).toBe("good");
    expect(imageTone("missing")).toBe("warning");
    expect(imageTone("unknown")).toBe("neutral");
    expect(imageTone()).toBe("neutral");
  });

  it("shortens a digest to twelve hex characters", () => {
    expect(shortImageId("sha256:0123456789abcdef0123")).toBe("0123456789ab");
    expect(shortImageId(null)).toBe("—");
  });

  it("joins image identity into one title", () => {
    expect(
      imageTitle({
        reference: "aibox:test",
        status: "built",
        id: "sha256:0123456789abcdef",
        created_at: "2026-08-18T01:02:03Z",
        size_bytes: 4_194_304,
        detail: null,
      }),
    ).toBe("aibox:test · 0123456789ab · 2026-08-18 09:02:03 · 4.0 MiB");
    expect(imageTitle()).toBe("Resolving image");
  });

  it("keeps Build primary and the cacheless action inline only while the image is not Built", () => {
    expect(buildActionTone("built")).toBe("secondary");
    expect(buildActionTone("missing")).toBe("primarySoft");
    expect(buildActionTone("unknown")).toBe("primarySoft");
    expect(buildActionTone()).toBe("secondary");
    expect(cachelessBuildInline("missing")).toBe(true);
    expect(cachelessBuildInline("unknown")).toBe(true);
    expect(cachelessBuildInline("built")).toBe(false);
    expect(cachelessBuildInline()).toBe(false);
  });
});
