import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { VisualConfigOptions } from "@/features/configs/editor/VisualConfigOptions";
import { claudeVisualOptions, configFile } from "@/features/configs/testFixtures";

describe("Visual Config fields", () => {
  it("names required fields and reveals credentials without changing their value", async () => {
    const onChange = vi.fn();
    render(
      <VisualConfigOptions
        file="settings.json"
        fields={configFile("settings.json", "{}", claudeVisualOptions()).visual_options!}
        onChange={onChange}
      />,
    );
    expect(screen.getByLabelText("Base URL")).toBeRequired();
    expect(screen.queryByRole("checkbox", { name: "Optional Base URL" })).toBeNull();
    expect(screen.getByText("env.ANTHROPIC_AUTH_TOKEN")).toBeVisible();
    const token = screen.getByLabelText("Auth token");
    expect(token).toBeRequired();
    expect(token).toHaveAttribute("type", "password");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Show Auth token" }));
    expect(token).toHaveAttribute("type", "text");
    expect(token).toHaveValue("secret");
    await user.click(screen.getByRole("button", { name: "Hide Auth token" }));
    expect(token).toHaveAttribute("type", "password");
    expect(onChange).not.toHaveBeenCalled();
  });

  it.each([
    {
      name: "declared value",
      value: "untrusted",
      included: true,
      required: true,
      options: ["untrusted", "never"],
    },
    {
      name: "stored unknown value",
      value: "future-policy",
      included: true,
      required: true,
      options: ["Unsupported: future-policy", "untrusted", "never"],
    },
    {
      name: "omitted optional value",
      value: undefined,
      included: false,
      required: false,
      options: ["untrusted", "never"],
    },
  ])("keeps enums closed for a $name", async ({ value, included, required, options }) => {
    const fields = configFile("config.toml", "", [
      {
        path: "approval_policy",
        label: "Approval policy",
        description: "Execution policy",
        group: "Execution",
        value_kind: "string",
        enum_values: ["untrusted", "never"],
        sensitive: false,
        required,
        included,
        value,
      },
    ]).visual_options!;
    const onChange = vi.fn();
    const view = render(
      <VisualConfigOptions file="config.toml" fields={fields} onChange={onChange} />,
    );
    const user = userEvent.setup();
    const input = screen.getByRole("combobox", { name: "Approval policy value" });
    if (!included) {
      expect(input).toBeDisabled();
      await user.click(screen.getByRole("checkbox", { name: "Optional Approval policy" }));
      expect(onChange).toHaveBeenLastCalledWith("approval_policy", {
        included: true,
        value: "untrusted",
      });
      view.rerender(
        <VisualConfigOptions
          file="config.toml"
          fields={[{ ...fields[0], included: true, value: "untrusted" }]}
          onChange={onChange}
        />,
      );
    } else {
      expect(input).toHaveTextContent(
        value === "future-policy" ? "Unsupported: future-policy" : value!,
      );
      expect(screen.queryByRole("checkbox", { name: "Optional Approval policy" })).toBeNull();
      expect(onChange).not.toHaveBeenCalled();
    }
    await user.click(input);
    const list = screen.getByRole("listbox", { name: "Approval policy single selection" });
    expect(
      within(list)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(options);
    await user.click(within(list).getByRole("option", { name: "never" }));
    expect(onChange).toHaveBeenLastCalledWith("approval_policy", {
      included: true,
      value: "never",
    });
  });
});
