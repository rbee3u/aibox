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
          "[1] user: 编译的时候好像要报这个问题，你看看能如何解决",
        ].join("\n"),
      ),
    ).toBe("编译的时候好像要报这个问题，你看看能如何解决");
    expect(
      messageNavigationLabel(
        "The following is the Codex agent history added since your last approval assessment. Continue the same review conversation.",
      ),
    ).toBe("Review continuation");
  });
});
