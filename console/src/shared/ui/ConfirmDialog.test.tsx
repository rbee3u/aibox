import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import actionButtonStyles from "@/shared/ui/ActionButton.module.css";

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

  /*
   * The typed name is the friction. A copy control beside it used to hand the
   * name over in one click; Enter, which the pattern promises, did nothing.
   */
  it("confirms on Enter once the typed name matches, and offers no copy shortcut", () => {
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
    expect(screen.queryByRole("button", { name: /copy/i })).not.toBeInTheDocument();

    fireEvent.change(input, { target: { value: "wor" } });
    fireEvent.submit(input.closest("form")!);
    expect(onConfirm).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: "work" } });
    fireEvent.submit(input.closest("form")!);
    expect(onConfirm).toHaveBeenCalledTimes(1);
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
        confirmLabel="Apply"
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
});
