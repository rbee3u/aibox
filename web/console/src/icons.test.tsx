import claudeIcon from "@lobehub/icons-static-svg/icons/claude.svg";
import openaiIcon from "@lobehub/icons-static-svg/icons/openai.svg";
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AgentIcon } from "./icons";

describe("AgentIcon", () => {
  it.each([
    ["claude", claudeIcon],
    ["codex", openaiIcon],
  ] as const)("uses the approved %s brand asset", (agent, source) => {
    const { container } = render(<AgentIcon agent={agent} size={15} />);
    const icon = container.querySelector<HTMLElement>(`[data-icon="${agent}"]`);

    expect(icon?.style.getPropertyValue("--brand-icon-size")).toBe("15px");
    expect(icon?.style.getPropertyValue("--brand-icon")).toBe(`url("${source}")`);
  });
});
