import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Operation } from "./controlApi";
import { OperationPanel } from "./managementTestSupport";

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
    expect(screen.getByText("Terminal state")).toBeInTheDocument();
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
});
