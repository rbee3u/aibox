import { render, screen } from "@testing-library/react";
import { RefreshCw } from "lucide-react";
import { describe, expect, it } from "vitest";
import { IconButton } from "@/shared/ui/IconButton";
import actionStyles from "@/shared/ui/ActionButton.module.css";

describe("IconButton", () => {
  it("uses its label as the accessible name without a visual tooltip", () => {
    render(
      <IconButton label="Refresh status" aria-pressed="true">
        <RefreshCw aria-hidden="true" />
      </IconButton>,
    );

    const button = screen.getByRole("button", { name: "Refresh status" });
    expect(button).toHaveAttribute("aria-pressed", "true");
    expect(button).toHaveAttribute("data-icon-button");
    expect(button).toHaveClass(actionStyles.ghost);
    expect(button).not.toHaveAttribute("title");
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("supports a quiet danger tone for inline destructive actions", () => {
    render(
      <IconButton label="Remove Component" tone="dangerQuiet">
        <RefreshCw aria-hidden="true" />
      </IconButton>,
    );

    expect(screen.getByRole("button", { name: "Remove Component" })).toHaveClass(
      actionStyles.dangerQuiet,
    );
  });
});
