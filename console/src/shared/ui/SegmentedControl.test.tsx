import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SegmentedControl } from "@/shared/ui/SegmentedControl";
import styles from "@/shared/ui/SegmentedControl.module.css";

const fileSystem = (
  globalThis as typeof globalThis & {
    process: {
      getBuiltinModule(name: "fs"): {
        readFileSync(path: string, encoding: "utf8"): string;
      };
    };
  }
).process.getBuiltinModule("fs");
const css = fileSystem.readFileSync("src/shared/ui/SegmentedControl.module.css", "utf8");

describe("SegmentedControl", () => {
  it("provides a shared filled mode-switch surface", () => {
    render(
      <SegmentedControl variant="filled" role="group" aria-label="Editor mode">
        <button type="button" aria-pressed="true">
          Visual
        </button>
        <button type="button" aria-pressed="false">
          Raw
        </button>
      </SegmentedControl>,
    );

    const group = screen.getByRole("group", { name: "Editor mode" });
    expect(group).toHaveClass(styles.root, styles.filled);
    expect(group.querySelectorAll("button")).toHaveLength(2);
  });

  it("provides a shared tab surface without changing tab semantics", () => {
    render(
      <SegmentedControl variant="tabs" role="tablist" aria-label="Request data">
        <button type="button" role="tab" aria-selected="true">
          Summary
        </button>
      </SegmentedControl>,
    );

    const tablist = screen.getByRole("tablist", { name: "Request data" });
    expect(tablist).toHaveClass(styles.root, styles.tabs);
    expect(screen.getByRole("tab", { name: "Summary" })).toHaveAttribute("aria-selected", "true");
  });

  it("uses the selected surface for the selected filled mode", () => {
    expect(css).toMatch(
      /\.filled > button\[aria-pressed="true"\][\s\S]*?background:\s*var\(--surface-selected\)/,
    );
    expect(css).toMatch(/\.filled > button\[aria-pressed="true"\][\s\S]*?color:\s*var\(--accent\)/);
  });
});
