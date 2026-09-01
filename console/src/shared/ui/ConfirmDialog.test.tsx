import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import actionButtonStyles from "@/shared/ui/ActionButton.module.css";
import styles from "@/shared/ui/ConfirmDialog.module.css";

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
          confirmLabel="Delete permanently"
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
    const confirm = within(dialog).getByRole("button", { name: "Delete permanently" });

    expect(cancel).toHaveClass(actionButtonStyles.secondary);
    expect(confirm).toHaveClass(actionButtonStyles.danger, styles.dangerAction);
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
        confirmLabel="Delete permanently"
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
        confirmLabel="Delete permanently"
        onConfirm={() => undefined}
        onCancel={() => undefined}
      />,
    );

    const input = screen.getByRole("textbox");
    const confirm = screen.getByRole("button", { name: "Delete permanently" });
    expect(input).toHaveFocus();
    expect(confirm).toBeDisabled();

    fireEvent.change(input, { target: { value: "work" } });
    expect(confirm).toBeEnabled();
  });

  it("copies the confirmation phrase without filling the input", async () => {
    vi.useFakeTimers();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });

    render(
      <ConfirmDialog
        title="Delete Tenant work?"
        confirmation="work"
        confirmLabel="Delete permanently"
        onConfirm={() => undefined}
        onCancel={() => undefined}
      />,
    );

    const phrase = screen.getByRole("button", { name: "Copy work" });
    const input = screen.getByRole("textbox");
    const confirm = screen.getByRole("button", { name: "Delete permanently" });

    await act(async () => {
      fireEvent.click(phrase);
      await Promise.resolve();
    });

    expect(writeText).toHaveBeenCalledWith("work");
    expect(input).toHaveValue("");
    expect(confirm).toBeDisabled();
    expect(screen.getByRole("button", { name: "Copied work" })).toBeInTheDocument();

    await act(() => vi.advanceTimersByTimeAsync(1400));
    expect(screen.getByRole("button", { name: "Copy work" })).toBeInTheDocument();
  });

  it("renders facts before the risk message", () => {
    render(
      <ConfirmDialog
        title="Apply openai to Current Config?"
        facts={[
          { label: "Tenant", value: "Host Tenant" },
          { label: "Coding Agent", value: "Codex" },
          { label: "Source", value: "Named Config openai" },
          { label: "Target", value: "Current Config" },
        ]}
        message="Present fields replace; omitted fixed fields are removed."
        confirmLabel="Apply to Current Config"
        variant="primary"
        onConfirm={() => undefined}
        onCancel={() => undefined}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "Apply openai to Current Config?" });
    expect(within(dialog).getByText("Tenant")).toBeInTheDocument();
    expect(within(dialog).getByText("Host Tenant")).toBeInTheDocument();
    expect(within(dialog).getByText("Coding Agent")).toBeInTheDocument();
    expect(within(dialog).getByText("Codex")).toBeInTheDocument();
    expect(dialog).toHaveTextContent("Present fields replace; omitted fixed fields are removed.");

    const facts = dialog.querySelector("dl");
    const message = within(dialog).getByText(
      "Present fields replace; omitted fixed fields are removed.",
    );
    expect(facts).not.toBeNull();
    expect(facts!.compareDocumentPosition(message) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
