import { describe, expect, it } from "vitest";
import { messageNavigationLabel } from "@/features/sessions/detail/sessionFormat";

describe("messageNavigationLabel", () => {
  it("uses the first readable line", () => {
    expect(messageNavigationLabel("First request\nSecond line")).toBe("First request");
  });

  it("collapses a leading skill file link to $name", () => {
    expect(
      messageNavigationLabel(
        "[$improve-unit-tests](/Users/rbee3u/.agents/skills/code-craft-skills/improve-unit-tests/SKILL.md)",
      ),
    ).toBe("$improve-unit-tests");
  });

  it("uses the embedded user line for a request-review prompt", () => {
    expect(
      messageNavigationLabel(
        [
          "The following is the Codex agent history whose request action you are assessing.",
          "",
          ">>> TRANSCRIPT START",
          "",
          "[1] user: The build seems to fail; please investigate",
        ].join("\n"),
      ),
    ).toBe("The build seems to fail; please investigate");
    expect(
      messageNavigationLabel(
        "The following is the Codex agent history added since your last approval assessment. Continue the same review conversation.",
      ),
    ).toBe("Review continuation");
  });
});
