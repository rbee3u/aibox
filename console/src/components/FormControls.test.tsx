import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TextInput, TextArea, Toggle } from "./FormControls";

describe("shared form controls", () => {
  it("preserves native value and ref behavior for text inputs", async () => {
    const user = userEvent.setup();
    const ref = { current: null as HTMLInputElement | null };
    render(<TextInput ref={ref} aria-label="Name" defaultValue="AIBox" />);

    const input = screen.getByRole("textbox", { name: "Name" });
    expect(ref.current).toBe(input);
    await user.clear(input);
    await user.type(input, "Console");
    expect(input).toHaveValue("Console");
  });

  it("emits a boolean change for toggles and exposes textarea semantics", async () => {
    const user = userEvent.setup();
    const changes: boolean[] = [];
    render(
      <>
        <Toggle aria-label="Include" onCheckedChange={(checked) => changes.push(checked)} />
        <TextArea aria-label="Content" />
      </>,
    );

    await user.click(screen.getByRole("checkbox", { name: "Include" }));
    expect(changes).toEqual([true]);
    expect(screen.getByRole("textbox", { name: "Content" })).toHaveAttribute(
      "data-aibox-control",
      "textarea",
    );
  });
});
