import claudeIcon from "@/shared/icons/brand/claude.svg";
import githubIcon from "@/shared/icons/brand/github.svg";
import goIcon from "@/shared/icons/brand/go.svg";
import nodejsIcon from "@/shared/icons/brand/nodejs.svg";
import openaiIcon from "@/shared/icons/brand/openai.svg";
import pythonIcon from "@/shared/icons/brand/python.svg";
import rustIcon from "@/shared/icons/brand/rust.svg";
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { moduleIcons, resourceIcons } from "@/shared/icons/consoleIcons";

describe("BrandIcon", () => {
  it.each([
    ["github", githubIcon],
    ["openai", openaiIcon],
    ["claude", claudeIcon],
    ["nodejs", nodejsIcon],
    ["python", pythonIcon],
    ["rust", rustIcon],
    ["go", goIcon],
  ] as const)("uses the registered %s brand asset and explicit size", (brand, source) => {
    const { container } = render(<BrandIcon brand={brand} size={19} />);
    const icon = container.querySelector<HTMLElement>(`[data-icon="${brand}"]`);

    expect(icon).toHaveAttribute("aria-hidden", "true");
    expect(icon?.style.getPropertyValue("--brand-icon-size")).toBe("19px");
    expect(icon?.style.getPropertyValue("--brand-icon")).toBe(`url("${source}")`);
  });

  it("maps Coding Agents to their brand identities", () => {
    expect(brandForAgent("codex")).toBe("openai");
    expect(brandForAgent("claude")).toBe("claude");
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
