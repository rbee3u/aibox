import { Inbox } from "lucide-react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import styles from "./EmptyState.module.css";
import { EmptyState } from "./EmptyState";

describe("EmptyState", () => {
  it("uses a compact non-heading title for list states", () => {
    const { container } = render(
      <EmptyState
        variant="list"
        icon={<Inbox data-icon="empty-list" aria-hidden="true" />}
        title="Nothing here"
        description="Try another scope."
      />,
    );

    const state = container.querySelector('[data-empty-state="list"]');
    expect(state).toHaveClass(styles.root, styles.list);
    expect(screen.getByText("Nothing here").tagName).toBe("STRONG");
    expect(screen.queryByRole("heading", { name: "Nothing here" })).not.toBeInTheDocument();
    expect(document.querySelector('[data-icon="empty-list"]')).toHaveClass("lucide-inbox");
  });

  it("uses a level-two heading for detail states and renders actions", () => {
    const { container } = render(
      <EmptyState
        variant="detail"
        icon={<Inbox aria-hidden="true" />}
        title="Choose a record"
        description="Select a row to continue."
      >
        <button type="button">Retry</button>
      </EmptyState>,
    );

    const state = container.querySelector('[data-empty-state="detail"]');
    expect(state).toHaveClass(styles.root, styles.detail);
    expect(screen.getByRole("heading", { level: 2, name: "Choose a record" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
});
