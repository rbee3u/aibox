import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { NativeSelect, TextInput, TextArea, Toggle } from "@/shared/ui/FormControls";
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
  it("emits a boolean change and keeps native toggle and textarea semantics", async () => {
    const user = userEvent.setup();
    const changes: boolean[] = [];
    render(
      <>
        <Toggle aria-label="Include" onCheckedChange={(checked) => changes.push(checked)}>
          Include
        </Toggle>
        <TextArea aria-label="Content" />
      </>,
    );
    await user.click(screen.getByRole("checkbox", { name: "Include" }));
    expect(changes).toEqual([true]);
    expect(screen.getByRole("checkbox", { name: "Include" })).toBeChecked();
    expect(screen.getByRole("textbox", { name: "Content" }).tagName).toBe("TEXTAREA");
  });
  it("preserves native disabled behavior for toggles", async () => {
    const user = userEvent.setup();
    const changes: boolean[] = [];
    render(
      <Toggle disabled aria-label="Disabled" onCheckedChange={(checked) => changes.push(checked)}>
        Disabled
      </Toggle>,
    );
    await user.click(screen.getByRole("checkbox", { name: "Disabled" }));
    expect(changes).toEqual([]);
    expect(screen.getByRole("checkbox", { name: "Disabled" })).not.toBeChecked();
  });
  it("preserves native select attributes and value changes", async () => {
    const user = userEvent.setup();
    render(
      <NativeSelect aria-label="Agent" defaultValue="codex" required>
        <option value="codex">Codex</option>
        <option value="claude">Claude</option>
      </NativeSelect>,
    );
    const select = screen.getByRole("combobox", { name: "Agent" });
    expect(select).toBeRequired();
    await user.selectOptions(select, "claude");
    expect(select).toHaveValue("claude");
  });
});
