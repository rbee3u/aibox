import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog, ContextPill } from "@/shared/ui/ConfirmDialog";
import actionButtonStyles from "@/shared/ui/ActionButton.module.css";
import confirmDialogStyles from "@/shared/ui/ConfirmDialog.module.css";

function DialogHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Delete selected
      </button>
      {open ? (
        <ConfirmDialog
          title="Delete selected record?"
          message="This cannot be undone."
          confirmLabel="Delete"
          onConfirm={() => undefined}
          onCancel={() => setOpen(false)}
        />
      ) : null}
    </>
  );
}

describe("ConfirmDialog", () => {
  it("traps focus and restores the trigger after Escape in Strict Mode", async () => {
    vi.useFakeTimers();
    render(
      <StrictMode>
        <DialogHarness />
      </StrictMode>,
    );

    const trigger = screen.getByRole("button", { name: "Delete selected" });
    trigger.focus();
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Delete selected record?" });
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    const confirm = within(dialog).getByRole("button", { name: "Delete" });

    expect(cancel).toHaveClass(actionButtonStyles.secondary);
    expect(confirm).toHaveClass(actionButtonStyles.dangerPrimary);
    expect(confirm.querySelector("svg")).toBeNull();
    expect(cancel).toHaveFocus();
    fireEvent.keyDown(cancel, { key: "Tab", shiftKey: true });
    expect(confirm).toHaveFocus();
    fireEvent.keyDown(confirm, { key: "Tab" });
    expect(cancel).toHaveFocus();

    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(dialog).not.toBeInTheDocument();
    await act(() => vi.runAllTimersAsync());
    expect(trigger).toHaveFocus();
  });

  it("locks cancellation while confirmation is pending", () => {
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        title="Delete selected record?"
        message="This cannot be undone."
        confirmLabel="Delete"
        onConfirm={() => undefined}
        onCancel={onCancel}
        busy
      />,
    );

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Deleting…" })).toBeDisabled();

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onCancel).not.toHaveBeenCalled();
    expect(dialog).toBeInTheDocument();
  });

  it("focuses and validates an explicit confirmation input", () => {
    render(
      <ConfirmDialog
        title="Delete Tenant work?"
        confirmation="work"
        confirmLabel="Delete"
        onConfirm={() => undefined}
        onCancel={() => undefined}
      />,
    );

    const input = screen.getByRole("textbox", { name: "Type work to confirm" });
    const confirm = screen.getByRole("button", { name: "Delete" });
    expect(input).toHaveFocus();
    expect(confirm).toBeDisabled();
    expect(input.closest("label")).toBeNull();

    fireEvent.change(input, { target: { value: "work" } });
    expect(confirm).toBeEnabled();
  });

  it("confirms on Enter once the typed name matches, and supports fill and copy shortcuts", async () => {
    vi.useFakeTimers();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });

    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        title="Delete Tenant work?"
        confirmation="work"
        confirmLabel="Delete"
        onConfirm={onConfirm}
        onCancel={() => undefined}
      />,
    );

    const input = screen.getByRole("textbox", { name: "Type work to confirm" });
    const confirm = screen.getByRole("button", { name: "Delete" });
    const fillButton = screen.getByRole("button", { name: "Fill work" });
    const copyButton = screen.getByRole("button", { name: "Copy work" });

    // Test copy shortcut
    await act(async () => {
      fireEvent.click(copyButton);
      await Promise.resolve();
    });
    expect(writeText).toHaveBeenCalledWith("work");
    expect(screen.getByRole("button", { name: "Copied work" })).toBeInTheDocument();

    await act(() => vi.advanceTimersByTimeAsync(1500));
    expect(screen.getByRole("button", { name: "Copy work" })).toBeInTheDocument();

    // Test fill shortcut
    expect(confirm).toBeDisabled();
    fireEvent.click(fillButton);
    expect(input).toHaveValue("work");
    expect(confirm).toBeEnabled();

    // Test Enter submit
    fireEvent.submit(input.closest("form")!);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("renders facts before the risk message", () => {
    render(
      <ConfirmDialog
        title="Apply openai to Current Config?"
        facts={[
          { label: "Tenant", value: "Host Tenant" },
          { label: "Agent", value: "Codex" },
          { label: "Source", value: "Named Config openai" },
          { label: "Target", value: "Current Config" },
        ]}
        message="Present fields replace; omitted fixed fields are removed."
        confirmLabel="Apply"
        variant="primary"
        onConfirm={() => undefined}
        onCancel={() => undefined}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "Apply openai to Current Config?" });
    expect(within(dialog).getByText("Tenant")).toBeInTheDocument();
    expect(within(dialog).getByText("Host Tenant")).toBeInTheDocument();
    expect(within(dialog).getByText("Agent")).toBeInTheDocument();
    expect(within(dialog).getByText("Codex")).toBeInTheDocument();
    expect(dialog).toHaveTextContent("Present fields replace; omitted fixed fields are removed.");
    expect(within(dialog).getByRole("button", { name: "Apply" })).toHaveClass(
      actionButtonStyles.primary,
    );

    const facts = dialog.querySelector("dl");
    const message = within(dialog).getByText(
      "Present fields replace; omitted fixed fields are removed.",
    );
    expect(facts).not.toBeNull();
    expect(facts!.compareDocumentPosition(message) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(getComputedStyle(within(dialog).getByText("Tenant")).textTransform).toBe("none");
  });

  it("uses an explicit busy label for domain-specific actions", () => {
    render(
      <ConfirmDialog
        title="Remove Component?"
        confirmLabel="Remove"
        busyLabel="Removing…"
        busy
        onConfirm={() => undefined}
        onCancel={() => undefined}
      />,
    );

    expect(screen.getByRole("button", { name: "Removing…" })).toBeDisabled();
  });

  it("applies fullWidthFact style to facts marked fullWidth", () => {
    render(
      <ConfirmDialog
        title="Delete this Request?"
        facts={[
          { label: "Request", value: "POST anyrouter.top/v1/messages", fullWidth: true },
          { label: "Status", value: "400" },
          { label: "Ended", value: "2026-03-14 11:42:07" },
          { label: "Id", value: "01a0a4ab-100c-7318-867e-7814afc31f42", fullWidth: true },
        ]}
        confirmLabel="Delete"
        onConfirm={() => undefined}
        onCancel={() => undefined}
      />,
    );

    const requestDt = screen.getByText("Request");
    const statusDt = screen.getByText("Status");
    const idDt = screen.getByText("Id");

    expect(requestDt.parentElement).toHaveClass(confirmDialogStyles.fullWidthFact);
    expect(statusDt.parentElement).not.toHaveClass(confirmDialogStyles.fullWidthFact);
    expect(idDt.parentElement).toHaveClass(confirmDialogStyles.fullWidthFact);
  });

  it("renders context pills with and without labels", () => {
    render(
      <ConfirmDialog
        title="Delete Named Config custom?"
        pills={
          <>
            <ContextPill label="Tenant" value="default" />
            <ContextPill label="Agent" value="Codex" />
            <ContextPill value="Isolated" />
          </>
        }
        confirmLabel="Delete"
        onConfirm={() => undefined}
        onCancel={() => undefined}
      />,
    );

    expect(screen.getByText("Tenant:")).toBeInTheDocument();
    expect(screen.getByText("default")).toBeInTheDocument();
    expect(screen.getByText("Agent:")).toBeInTheDocument();
    expect(screen.getByText("Codex")).toBeInTheDocument();
    expect(screen.getByText("Isolated")).toBeInTheDocument();
  });
});
