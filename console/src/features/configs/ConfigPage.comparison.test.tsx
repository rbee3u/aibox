import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import type { ConfigComparison, ConfigListData } from "@/api/configs";
import { configFile, claudeVisualOptions } from "@/features/configs/testFixtures";
import { ConfigPage, configApi } from "@/features/configs/testHarness";

const catalog: ConfigListData = {
  configs: [
    { name: "team", state: "ready" },
    { name: "other", state: "ready" },
  ],
  files: ["settings.json"],
  application: {
    last_application: { applied: "team", applied_at: "2026-09-01T00:00:00Z" },
    drift: "dirty",
  },
  credential_propagation_available: false,
};
function compared(value = "https://different.test"): ConfigComparison {
  return {
    source: "team",
    incomplete: false,
    files: [
      {
        file: "settings.json",
        error: null,
        named: { revision: "settings.json-revision", exists: true, content: "{}" },
        current: { revision: "current", exists: true, content: "{}" },
        differences: [
          {
            path: ["env", "ANTHROPIC_BASE_URL"],
            named_present: true,
            current_present: true,
            named_value: "https://example.com",
            current_value: value,
            sensitive: false,
            named_range: null,
            current_range: null,
          },
        ],
      },
    ],
  };
}
function open(name = "team") {
  window.history.replaceState(
    null,
    "",
    `/_aibox/ui/configs?tenant=host&agent=claude&config=${name}`,
  );
}
afterEach(() => window.history.replaceState(null, "", "/"));

describe("Config comparison", () => {
  it("marks Visual fields, expands named/current values, and sends live Visual drafts", async () => {
    open();
    const { api, compareConfigs } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () =>
        Promise.resolve(configFile("settings.json", "{}", claudeVisualOptions())),
      compareConfigs: () => Promise.resolve(compared()),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    const summary = await screen.findByText("Differs from Current Config");
    await user.click(summary);
    expect(within(summary.parentElement!).getByText("Current Config")).toBeVisible();
    expect(within(summary.parentElement!).getByText('"https://different.test"')).toBeVisible();
    const input = screen.getByRole("textbox", { name: "Base URL" });
    await user.clear(input);
    await user.type(input, "https://draft.test");
    await waitFor(() =>
      expect(
        compareConfigs.mock.lastCall?.[1][0].visualOptions?.find(
          (option) => option.path === "env.ANTHROPIC_BASE_URL",
        )?.value,
      ).toBe("https://draft.test"),
    );
    expect(await screen.findByText(/1 difference · Unsaved content/)).toBeVisible();
    expect(screen.getByText("Differs")).not.toHaveAttribute("title");
  });

  it("clears obsolete results during invalid Raw drafts and recovers", async () => {
    open();
    const { api } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () => Promise.resolve(configFile("settings.json", "{}")),
      diagnoseConfigFile: () => Promise.resolve({ diagnostics: [] }),
      compareConfigs: (_target, files) =>
        Promise.resolve(
          atob(files[0].contentBase64) === "{"
            ? {
                source: "team",
                incomplete: true,
                files: [
                  {
                    file: "settings.json",
                    error: "Invalid JSON",
                    named: null,
                    current: null,
                    differences: [],
                  },
                ],
              }
            : compared(),
        ),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await screen.findByText("1 difference", { selector: "summary" });
    const input = screen.getByRole("textbox", { name: "settings.json content" });
    await user.clear(input);
    await user.type(input, "{{");
    await screen.findByText("Comparison unavailable: Invalid JSON");
    expect(screen.queryByText("1 difference", { selector: "summary" })).not.toBeInTheDocument();
    await user.type(input, "}");
    expect(screen.queryByText("1 difference", { selector: "summary" })).not.toBeInTheDocument();
    expect(await screen.findByText("1 difference", { selector: "summary" })).toBeVisible();
  });

  it("does not compare unrelated Named Configs", async () => {
    open("other");
    const { api, compareConfigs } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () =>
        Promise.resolve(configFile("settings.json", "{}", claudeVisualOptions())),
    });
    render(<ConfigPage api={api} />);
    await screen.findByRole("textbox", { name: "Base URL" });
    expect(compareConfigs).not.toHaveBeenCalled();
    expect(screen.queryByText(/Comparing/)).not.toBeInTheDocument();
  });

  it("ignores an old response after a newer draft has been compared", async () => {
    open();
    let resolveOld!: (result: ConfigComparison) => void;
    let calls = 0;
    const { api, compareConfigs } = configApi({
      listConfigs: () => Promise.resolve(catalog),
      revealConfigFile: () => Promise.resolve(configFile("settings.json", "{}")),
      diagnoseConfigFile: () => Promise.resolve({ diagnostics: [] }),
      compareConfigs: () =>
        ++calls === 1
          ? new Promise((resolve) => {
              resolveOld = resolve;
            })
          : Promise.resolve(compared("latest")),
    });
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);
    await waitFor(() => expect(compareConfigs).toHaveBeenCalledTimes(1));
    await user.type(screen.getByRole("textbox", { name: "settings.json content" }), " ");
    await screen.findByText(/1 difference/, { selector: "summary" });
    resolveOld(compared("obsolete"));
    await user.click(screen.getByText(/1 difference/, { selector: "summary" }));
    await user.click(screen.getByText("env.ANTHROPIC_BASE_URL"));
    expect(await screen.findByText('"latest"')).toBeVisible();
    expect(screen.queryByText('"obsolete"')).not.toBeInTheDocument();
  });
});

it("compares ChatGPT credentials as native content and counts their internal differences once", async () => {
  window.history.replaceState(null, "", "/_aibox/ui/configs?tenant=host&agent=codex&config=team");
  const authText =
    '{"auth_mode":"chatgpt","tokens":{"account_id":"account","access_token":"secret"},"last_refresh":"2026-09-01T00:00:00Z"}';
  const { api, compareConfigs } = configApi({
    listConfigs: () => Promise.resolve({ ...catalog, files: ["config.toml", "auth.json"] }),
    revealConfigFile: (target) =>
      Promise.resolve(
        target.file === "auth.json"
          ? {
              ...configFile("auth.json", authText),
              auth: { mode: "chatgpt", api_key: null, extra_fields: false, warnings: [] },
            }
          : configFile("config.toml", "", []),
      ),
    compareConfigs: () =>
      Promise.resolve({
        source: "team",
        incomplete: false,
        files: [
          {
            file: "config.toml",
            error: null,
            named: { content: "", revision: "r", exists: true },
            current: { content: "", revision: "c", exists: true },
            differences: [],
          },
          {
            file: "auth.json",
            error: null,
            named: { content: authText, revision: "r", exists: true },
            current: { content: authText, revision: "c", exists: true },
            differences: [
              {
                path: ["tokens", "access_token"],
                named_present: true,
                current_present: true,
                named_value: "secret",
                current_value: "changed",
                sensitive: true,
                named_range: null,
                current_range: null,
              },
              {
                path: ["last_refresh"],
                named_present: true,
                current_present: true,
                named_value: "before",
                current_value: "after",
                sensitive: true,
                named_range: null,
                current_range: null,
              },
            ],
          },
        ],
      }),
  });
  const user = userEvent.setup();
  render(<ConfigPage api={api} />);
  await screen.findByText("Differs from Current credentials");
  const authDraft = compareConfigs.mock.lastCall?.[1].find((file) => file.file === "auth.json");
  expect(authDraft?.visualAuth).toBeUndefined();
  expect(atob(authDraft!.contentBase64)).toBe(authText);
  expect(screen.getByRole("button", { name: "Show 1 differences for auth.json" })).toBeVisible();
  await user.click(screen.getByText("1 difference", { selector: "summary" }));
  const path = screen.getAllByText("tokens.access_token", { selector: "code" })[0];
  await user.click(path);
  expect(screen.queryByText('"secret"')).not.toBeInTheDocument();
  await user.click(within(path.closest("details")!).getByRole("button", { name: "Show values" }));
  expect(screen.getByText('"secret"')).toBeVisible();
});

it("protects Visual drafts before locating a difference in Raw", async () => {
  open();
  const { api } = configApi({
    listConfigs: () => Promise.resolve(catalog),
    revealConfigFile: () =>
      Promise.resolve(configFile("settings.json", "{}", claudeVisualOptions())),
    compareConfigs: () => Promise.resolve(compared()),
  });
  const user = userEvent.setup();
  render(<ConfigPage api={api} />);
  const input = await screen.findByRole("textbox", { name: "Base URL" });
  await user.type(input, "/draft");
  const summary = await screen.findByText(/1 difference · Unsaved content/, {
    selector: "summary",
  });
  await user.click(summary);
  await user.click(screen.getAllByText("env.ANTHROPIC_BASE_URL", { selector: "code" })[0]);
  await user.click(screen.getByRole("button", { name: "Locate in Raw" }));
  const dialog = await screen.findByRole("dialog", { name: "Unsaved changes" });
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.getByRole("button", { name: "Visual" })).toHaveAttribute("aria-pressed", "true");
  expect(input).toHaveValue("https://example.com/draft");
});

it("Refresh rereads the selected file and compares the new snapshot", async () => {
  open();
  let revision = "first";
  const { api, revealConfigFile, compareConfigs } = configApi({
    listConfigs: () => Promise.resolve({ ...catalog }),
    revealConfigFile: () =>
      Promise.resolve({ ...configFile("settings.json", "{}", claudeVisualOptions()), revision }),
    compareConfigs: () => Promise.resolve(compared()),
  });
  const user = userEvent.setup();
  render(<ConfigPage api={api} />);
  await screen.findByText("Differs from Current Config");
  revision = "second";
  await user.click(screen.getByRole("button", { name: "Refresh Configs" }));
  await waitFor(() => expect(revealConfigFile).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(compareConfigs.mock.lastCall?.[1][0].revision).toBe("second"));
});
