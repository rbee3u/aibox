import type { ComponentProps } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Operation } from "@/api/operations";
import { OperationPanel as OperationPanelView } from "@/app/OperationPanel";
import { composeTestApi, type TestControlApi } from "@/test/controlApi";

function OperationPanel(
  props: Omit<ComponentProps<typeof OperationPanelView>, "api"> & { api: TestControlApi },
) {
  const { api, ...panelProps } = props;
  return <OperationPanelView {...panelProps} api={composeTestApi(api).operations} />;
}

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("OperationPanel", () => {
  const runningOperation: Operation = {
    id: "operation-1",
    kind: "Install Rust toolchain",
    state: "running",
    started_at: "2026-08-19T01:00:00Z",
    ended_at: null,
    result: null,
    first_sequence: 2,
    next_sequence: 4,
    logs: [
      { sequence: 2, message: "Downloading" },
      { sequence: 3, message: "Installing" },
    ],
  };
  it("reports cancellation immediately and prevents duplicate requests", async () => {
    const post = vi.fn().mockResolvedValue({});
    const api = { post, get: vi.fn() };
    const user = userEvent.setup();
    render(
      <OperationPanel
        api={api}
        operation={runningOperation}
        connection="reconnecting"
        onOperation={() => undefined}
        onDismiss={() => undefined}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Cancel operation" }));
    expect(screen.getByText("Cancellation requested")).toBeInTheDocument();
    expect(screen.getByText("Reconnecting to live updates")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancellation requested" })).toBeDisabled();
    expect(post).toHaveBeenCalledTimes(1);
    expect(post).toHaveBeenCalledWith("/_aibox/api/operations/operation-1/cancel");
  });
  it("shows log gaps and keeps failed Operations expanded", () => {
    const api = { post: vi.fn(), get: vi.fn() };
    render(
      <OperationPanel
        api={api}
        operation={{
          ...runningOperation,
          state: "failed",
          ended_at: "2026-08-19T01:01:00Z",
          result: "Docker exited with status 1",
        }}
        onOperation={() => undefined}
        onDismiss={() => undefined}
      />,
    );
    expect(screen.getByText(/Earlier log output was truncated/)).toBeInTheDocument();
    expect(document.querySelector("pre")).toHaveTextContent(/Downloading\s+Installing/);
    expect(screen.getByText("Finished")).toBeInTheDocument();
    // The reason reads under the state that names it, not in the footer.
    const result = screen.getByText("Docker exited with status 1");
    expect(result.closest("footer")).toBeNull();
    expect(result.closest("[role='status']")).not.toBeNull();
    expect(document.querySelector("pre")).not.toHaveTextContent("Docker exited with status 1");
    expect(screen.getByRole("button", { name: "Collapse operation" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });
  it("respects a user collapse across polling and expands a new failure", async () => {
    const api = { post: vi.fn(), get: vi.fn() };
    const user = userEvent.setup();
    const view = render(
      <OperationPanel
        api={api}
        operation={runningOperation}
        onOperation={() => undefined}
        onDismiss={() => undefined}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Collapse operation" }));
    view.rerender(
      <OperationPanel
        api={api}
        operation={{
          ...runningOperation,
          next_sequence: 5,
          logs: [...runningOperation.logs, { sequence: 4, message: "Still installing" }],
        }}
        onOperation={() => undefined}
        onDismiss={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: "Expand operation" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    view.rerender(
      <OperationPanel
        api={api}
        operation={{
          ...runningOperation,
          id: "operation-2",
          state: "failed",
          ended_at: "2026-08-19T01:02:00Z",
          result: "Installation failed",
        }}
        onOperation={() => undefined}
        onDismiss={() => undefined}
      />,
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Collapse operation" })).toHaveAttribute(
        "aria-expanded",
        "true",
      ),
    );
  });
  it("distinguishes the three terminal states by tone, mark, and reading", () => {
    const api = { post: vi.fn(), get: vi.fn() };
    // Failure and cancellation shared one stop-sign mark and one muted grey, so
    // a failed install and a successful one were indistinguishable at a glance.
    const cases = [
      { state: "succeeded", tone: "good", label: "Succeeded", mark: "lucide-check" },
      { state: "failed", tone: "error", label: "Failed", mark: "lucide-circle-x" },
      { state: "cancelled", tone: "warning", label: "Cancelled", mark: "lucide-ban" },
    ] as const;
    const seen = new Set<string>();
    for (const { state, tone, label, mark } of cases) {
      const view = render(
        <OperationPanel
          api={api}
          operation={{ ...runningOperation, state, ended_at: "2026-08-19T01:03:07Z" }}
          onOperation={() => undefined}
          onDismiss={() => undefined}
        />,
      );
      const badge = screen.getByText(label);
      expect(badge.closest("[data-status-tone]")).toHaveAttribute("data-status-tone", tone);
      expect(document.querySelector(`header .${mark}`)).toBeInTheDocument();
      seen.add(tone);
      seen.add(mark);
      view.unmount();
    }
    expect(seen.size).toBe(6);
  });
  it("reports how long a running Operation has taken, and how long a finished one took", () => {
    const api = { post: vi.fn(), get: vi.fn() };
    vi.useFakeTimers();
    try {
      vi.setSystemTime(Date.parse("2026-08-19T01:03:07Z"));
      const view = render(
        <OperationPanel
          api={api}
          operation={runningOperation}
          onOperation={() => undefined}
          onDismiss={() => undefined}
        />,
      );
      // started_at is 01:00:00, so a multi-minute install can be told apart
      // from one that has just stalled.
      expect(screen.getByText("3m7s")).toBeInTheDocument();
      view.unmount();
      render(
        <OperationPanel
          api={api}
          operation={{
            ...runningOperation,
            state: "succeeded",
            ended_at: "2026-08-19T01:04:12Z",
          }}
          onOperation={() => undefined}
          onDismiss={() => undefined}
        />,
      );
      expect(screen.getByText("took 4m12s")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
  it("reports the height it occupies so the shell can reserve it", () => {
    const api = { post: vi.fn(), get: vi.fn() };
    const heights: number[] = [];
    // A fixed panel over an unpadded workspace put the last catalog row out of
    // reach, with no scroll available to recover it.
    const view = render(
      <OperationPanel
        api={api}
        operation={runningOperation}
        onOperation={() => undefined}
        onDismiss={() => undefined}
        onHeightChange={(height) => heights.push(height)}
      />,
    );
    expect(heights.length).toBeGreaterThan(0);
    view.unmount();
    expect(heights[heights.length - 1]).toBe(0);
  });
});
