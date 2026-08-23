import claudeIcon from "./assets/brand/claude.svg";
import openaiIcon from "./assets/brand/openai.svg";
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AgentIcon } from "./icons";
import { moduleIcons, resourceIcons } from "./consoleIcons";

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

describe("Console icon registry", () => {
  it("uses one Session icon across the module and resource vocabulary", () => {
    expect(moduleIcons.sessions).toBe(resourceIcons.session);

    const ModuleIcon = moduleIcons.sessions;
    const ResourceIcon = resourceIcons.session;
    const { container } = render(
      <>
        <ModuleIcon data-icon="sessions-module" />
        <ResourceIcon data-icon="session-resource" />
      </>,
    );

    expect(container.querySelector('[data-icon="sessions-module"]')).toHaveClass(
      "lucide-messages-square",
    );
    expect(container.querySelector('[data-icon="session-resource"]')).toHaveClass(
      "lucide-messages-square",
    );
  });
});
