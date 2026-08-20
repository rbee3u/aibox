import { render, screen } from "@testing-library/react";
import { RefreshCw } from "lucide-react";
import { describe, expect, it } from "vitest";
import { IconButton } from "./IconButton";

describe("IconButton", () => {
  it("uses its tooltip label as the accessible name", () => {
    render(
      <IconButton label="Refresh status" aria-pressed="true">
        <RefreshCw aria-hidden="true" />
      </IconButton>,
    );

    const button = screen.getByRole("button", { name: "Refresh status" });
    expect(button).toHaveAttribute("title", "Refresh status");
    expect(button).toHaveAttribute("aria-pressed", "true");
    expect(button).toHaveAttribute("data-icon-button");
  });
});
