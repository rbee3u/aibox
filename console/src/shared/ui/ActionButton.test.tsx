import { createRef } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ActionButton } from "@/shared/ui/ActionButton";
import styles from "@/shared/ui/ActionButton.module.css";

const fileSystem = (
  globalThis as typeof globalThis & {
    process: {
      getBuiltinModule(name: "fs"): {
        readFileSync(path: string, encoding: "utf8"): string;
      };
    };
  }
).process.getBuiltinModule("fs");
const css = fileSystem.readFileSync("src/shared/ui/ActionButton.module.css", "utf8");

describe("ActionButton", () => {
  it("maps tones to the AIBox-owned native action contract", () => {
    render(
      <>
        <ActionButton>Default</ActionButton>
        <ActionButton tone="primary">Create</ActionButton>
        <ActionButton tone="primarySoft">Apply</ActionButton>
        <ActionButton tone="primarySoft">Create quietly</ActionButton>
        <ActionButton tone="secondary">Create secondary</ActionButton>
        <ActionButton tone="dangerQuiet">Remove inline</ActionButton>
        <ActionButton tone="danger">Delete</ActionButton>
        <ActionButton tone="dangerPrimary">Delete permanently</ActionButton>
        <ActionButton tone="ghost">More</ActionButton>
      </>,
    );

    expect(screen.getByRole("button", { name: "Default" })).toHaveClass(styles.secondary);
    expect(screen.getByRole("button", { name: "Create" })).toHaveClass(styles.primary);
    expect(screen.getByRole("button", { name: "Apply" })).toHaveClass(styles.primarySoft);
    expect(screen.getByRole("button", { name: "Create quietly" })).toHaveClass(styles.primarySoft);
    expect(screen.getByRole("button", { name: "Create secondary" })).toHaveClass(styles.secondary);
    expect(screen.getByRole("button", { name: "Remove inline" })).toHaveClass(styles.dangerQuiet);
    expect(screen.getByRole("button", { name: "Delete" })).toHaveClass(styles.danger);
    expect(screen.getByRole("button", { name: "Delete permanently" })).toHaveClass(
      styles.dangerPrimary,
    );
    expect(screen.getByRole("button", { name: "More" })).toHaveClass(styles.ghost);
    expect(screen.getByRole("button", { name: "Apply" })).toHaveAttribute("type", "button");
    expect(styles.primary).not.toEqual(styles.primarySoft);
    expect(styles.primarySoft).not.toEqual(styles.secondary);
    expect(styles.secondary).not.toEqual(styles.ghost);
    expect(styles.ghost).not.toEqual(styles.dangerQuiet);
    expect(styles.dangerQuiet).not.toEqual(styles.danger);
    expect(styles.dangerPrimary).not.toEqual(styles.danger);
    expect(styles.ghost).not.toEqual(styles.danger);
  });

  it("keeps the quiet primary and danger variants visually distinct", () => {
    expect(css).toMatch(
      /\.primary\s*\{[^}]*color:\s*var\(--action-primary-ink\)[^}]*background:\s*var\(--action-primary-surface\)/s,
    );
    expect(css).toMatch(
      /\.primary:hover:not\(:disabled\)\s*\{[^}]*background:\s*var\(--action-primary-hover-surface\)/s,
    );
    expect(css).toMatch(
      /\.primarySoft\s*\{[^}]*border-color:\s*var\(--action-primary-soft-line\)[^}]*color:\s*var\(--action-primary-soft-ink\)[^}]*background:\s*var\(--action-primary-soft-surface\)/s,
    );
    expect(css).toMatch(
      /\.primarySoft:hover:not\(:disabled\)\s*\{[^}]*border-color:\s*var\(--action-primary-soft-hover-line\)[^}]*color:\s*var\(--action-primary-soft-hover-ink\)[^}]*background:\s*var\(--action-primary-soft-hover-surface\)/s,
    );
    expect(css).toMatch(/\.secondary\s*\{[^}]*border-color:\s*var\(--line-control\)/s);
    expect(css).toMatch(/\.ghost\s*\{[^}]*background:\s*transparent/s);
    expect(css).toMatch(
      /\.ghost:hover:not\(:disabled\)\s*\{[^}]*color:\s*var\(--ink-secondary\)[^}]*background:\s*var\(--surface-hover\)/s,
    );
    expect(css).toMatch(/\.dangerQuiet\s*\{[^}]*color:\s*var\(--ink-secondary\)/s);
    expect(css).toMatch(
      /\.dangerQuiet:hover:not\(:disabled\)[^}]*color:\s*var\(--danger-strong\)/s,
    );
    expect(css).toMatch(/\.danger\s*\{[^}]*background:\s*transparent/s);
    expect(css).toMatch(
      /\.dangerPrimary\s*\{[^}]*border-color:\s*var\(--danger-line\)[^}]*color:\s*var\(--action-danger\)[^}]*background:\s*transparent/s,
    );
    expect(css).toMatch(
      /\.dangerPrimary:hover:not\(:disabled\)\s*\{[^}]*border-color:\s*var\(--action-danger-strong\)[^}]*color:\s*var\(--action-danger-strong\)[^}]*background:\s*var\(--danger-soft\)/s,
    );
  });

  it("keeps disabled variants identifiable without whole-control fading", () => {
    expect(css).toMatch(/\.button:disabled\s*\{[^}]*opacity:\s*1/s);
    expect(css).toMatch(
      /\.primary:disabled,\s*\.primarySoft:disabled\s*\{[^}]*border-color:\s*var\(--control-disabled-line\)[^}]*color:\s*var\(--control-disabled-ink\)[^}]*background:\s*var\(--control-disabled-surface\)/s,
    );
    expect(css).toMatch(
      /\.dangerPrimary:disabled\s*\{[^}]*color:\s*var\(--control-disabled-danger-ink\)[^}]*background:\s*var\(--control-disabled-danger-surface\)/s,
    );
    expect(css).toMatch(
      /\.secondary:disabled\s*\{[^}]*border-color:\s*var\(--control-disabled-line\)[^}]*color:\s*var\(--control-disabled-ink\)[^}]*background:\s*var\(--control-disabled-surface\)/s,
    );
    expect(css).toMatch(
      /\.ghost:disabled,[^}]*\.dangerQuiet:disabled,[^}]*\.danger:disabled\s*\{[^}]*color:\s*var\(--control-disabled-ink\)[^}]*background:\s*transparent/s,
    );
  });

  it("supports native form submission semantics", async () => {
    const user = userEvent.setup();
    const ref = createRef<HTMLButtonElement>();
    let submitted = false;
    render(
      <form
        onSubmit={(event) => {
          event.preventDefault();
          submitted = true;
        }}
      >
        <ActionButton ref={ref} type="submit" tone="primarySoft">
          Save
        </ActionButton>
      </form>,
    );

    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(ref.current).toBe(screen.getByRole("button", { name: "Save" }));
    expect(submitted).toBe(true);
  });
});
