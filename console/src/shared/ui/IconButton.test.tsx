import { act, fireEvent, render, screen } from "@testing-library/react";
import { RefreshCw } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import { IconButton } from "@/shared/ui/IconButton";

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

  it("opens its owned tooltip after hover delay and immediately on focus", () => {
    vi.useFakeTimers();
    render(
      <IconButton label="Refresh status">
        <RefreshCw aria-hidden="true" />
      </IconButton>,
    );
    const button = screen.getByRole("button", { name: "Refresh status" });

    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(449);
    });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByRole("tooltip")).toHaveTextContent("Refresh status");

    fireEvent.pointerLeave(button);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    fireEvent.focus(button);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    fireEvent.keyDown(button, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    expect(button).toHaveFocus();
  });

  it("supports touch-style opening and outside dismissal", () => {
    render(
      <IconButton label="Refresh status">
        <RefreshCw aria-hidden="true" />
      </IconButton>,
    );
    const button = screen.getByRole("button", { name: "Refresh status" });

    fireEvent.pointerDown(button, { pointerType: "touch" });
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });
});
