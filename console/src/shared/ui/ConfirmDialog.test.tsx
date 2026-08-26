import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";

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
});
