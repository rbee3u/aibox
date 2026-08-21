import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ConsoleProvider } from "../ConsoleProvider";
import { ActionButton } from "./ActionButton";

describe("ActionButton", () => {
  it("maps primary and danger tones to the shared Ant Design action contract", () => {
    render(
      <ConsoleProvider>
        <ActionButton tone="primary">Apply</ActionButton>
        <ActionButton tone="danger">Delete</ActionButton>
      </ConsoleProvider>,
    );

    expect(screen.getByRole("button", { name: "Apply" })).toHaveClass("aibox-btn-primary");
    expect(screen.getByRole("button", { name: "Delete" })).toHaveClass("aibox-btn-dangerous");
    expect(screen.getByRole("button", { name: "Apply" })).toHaveAttribute(
      "data-aibox-control",
      "button",
    );
  });

  it("supports native form submission semantics", async () => {
    const user = userEvent.setup();
    let submitted = false;
    render(
      <ConsoleProvider>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            submitted = true;
          }}
        >
          <ActionButton htmlType="submit" tone="primary">
            Save
          </ActionButton>
        </form>
      </ConsoleProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(submitted).toBe(true);
  });
});
