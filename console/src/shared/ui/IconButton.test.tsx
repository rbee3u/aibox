import { act, fireEvent, render, screen } from "@testing-library/react";
import { RefreshCw } from "lucide-react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { IconButton } from "@/shared/ui/IconButton";
import actionStyles from "@/shared/ui/ActionButton.module.css";

afterEach(() => {
  vi.useRealTimers();
});

describe("IconButton", () => {
  it("uses its label as the accessible name", () => {
    render(
      <IconButton label="Refresh status" aria-pressed="true">
        <RefreshCw aria-hidden="true" />
      </IconButton>,
    );

    const button = screen.getByRole("button", { name: "Refresh status" });
    expect(button).toHaveAttribute("aria-pressed", "true");
    expect(button).toHaveAttribute("data-icon-button");
    expect(button).toHaveClass(actionStyles.ghost);
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

  /*
   * A pointer waits and a keyboard does not. Both have to arrive somewhere,
   * because the icon is the whole of what either one can see.
   */
  it("opens its tooltip after a hover delay and immediately on focus", () => {
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
    expect(button).toHaveAttribute("aria-describedby", screen.getByRole("tooltip").id);

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

  it("stays quiet while disabled, and still forwards a ref", () => {
    vi.useFakeTimers();
    let element: HTMLButtonElement | null = null;
    render(
      <IconButton
        label="Refresh status"
        disabled
        ref={(node) => {
          element = node;
        }}
      >
        <RefreshCw aria-hidden="true" />
      </IconButton>,
    );
    const button = screen.getByRole("button", { name: "Refresh status" });

    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    expect(element).toBe(button);
  });
});
