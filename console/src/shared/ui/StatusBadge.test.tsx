import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import styles from "@/shared/ui/StatusBadge.module.css";

describe("StatusBadge", () => {
  it("uses a shared tone and emphasis variant", () => {
    render(
      <StatusBadge tone="good" variant="badge">
        Healthy
      </StatusBadge>,
    );
    const badge = screen.getByText("Healthy").parentElement!;
    expect(badge).toHaveClass(styles.badge, styles.good);
    expect(badge).toHaveAttribute("data-status-tone", "good");
    expect(badge).toHaveAttribute("data-status-variant", "badge");
  });

  it("uses a dot for lightweight inline statuses", () => {
    render(
      <StatusBadge tone="active" variant="inline">
        Streaming
      </StatusBadge>,
    );
    const status = screen.getByText("Streaming").parentElement!;
    expect(status).toHaveClass(styles.inline, styles.active);
    expect(status.querySelector(`.${styles.dot}`)).toBeInTheDocument();
  });
});
