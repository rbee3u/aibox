import { createRef } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ActionButton } from "./ActionButton";
import styles from "./ActionButton.module.css";

describe("ActionButton", () => {
  it("maps tones to the AIBox-owned native action contract", () => {
    render(
      <>
        <ActionButton>Default</ActionButton>
        <ActionButton tone="primary">Apply</ActionButton>
        <ActionButton tone="danger">Delete</ActionButton>
        <ActionButton tone="quiet">More</ActionButton>
      </>,
    );

    expect(screen.getByRole("button", { name: "Default" })).toHaveClass(styles.default);
    expect(screen.getByRole("button", { name: "Apply" })).toHaveClass(styles.primary);
    expect(screen.getByRole("button", { name: "Delete" })).toHaveClass(styles.danger);
    expect(screen.getByRole("button", { name: "More" })).toHaveClass(styles.quiet);
    expect(screen.getByRole("button", { name: "Apply" })).toHaveAttribute("type", "button");
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
        <ActionButton ref={ref} type="submit" tone="primary">
          Save
        </ActionButton>
      </form>,
    );

    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(ref.current).toBe(screen.getByRole("button", { name: "Save" }));
    expect(submitted).toBe(true);
  });
});
