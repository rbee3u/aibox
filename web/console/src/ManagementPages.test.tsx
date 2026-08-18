import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ControlApi } from "./controlApi";
import type { ConfigListData, SessionListData, SessionRow, TenantRow } from "./controlApi";
import { ConfigPage, SessionPage } from "./ManagementPages";
import styles from "./ManagementPages.module.css";
import { deferred } from "./test/fixtures";

const firstSession = {
  id: "11111111-1111-1111-1111-111111111111",
  display_id: "111111111111",
  start_ts: "2026-08-17T09:00:00Z",
  title: "First prompt",
  warnings: [],
} satisfies SessionRow;

const secondSession = {
  id: "22222222-2222-2222-2222-222222222222",
  display_id: "222222222222",
  start_ts: "2026-08-17T08:00:00Z",
  title: "Second prompt",
  warnings: [],
} satisfies SessionRow;

const thirdSession = {
  id: "33333333-3333-3333-3333-333333333333",
  display_id: "333333333333",
  start_ts: "2026-08-17T07:00:00Z",
  title: "New prompt",
  warnings: [],
} satisfies SessionRow;

const tenants = [
  {
    kind: "host",
    name: null,
    display_name: "Host Tenant",
    home: "/home/test",
    exists: true,
  },
  {
    kind: "managed",
    name: "work",
    display_name: "work",
    home: "/aibox/tenants/work",
    exists: true,
  },
] satisfies TenantRow[];

function list(sessions: SessionRow[], warnings: string[] = []): SessionListData {
  return {
    sessions,
    warnings,
    partial: warnings.length > 0 || sessions.some((session) => session.warnings.length > 0),
  };
}

function fakeApi({
  sessions = () => list([firstSession, secondSession]),
  post = vi.fn().mockResolvedValue({ deleted: 1 }),
  streamSession = vi.fn().mockResolvedValue({ id: firstSession.id, warnings: [] }),
}: {
  sessions?: (path: string, signal?: AbortSignal) => Promise<SessionListData> | SessionListData;
  post?: ReturnType<typeof vi.fn>;
  streamSession?: ReturnType<typeof vi.fn>;
} = {}) {
  const get = vi.fn((path: string, signal?: AbortSignal) => {
    if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
    if (path.startsWith("/_aibox/api/sessions?")) return Promise.resolve(sessions(path, signal));
    return Promise.reject(new Error(`Unexpected GET ${path}`));
  });
  const api = {
    bootstrap: { version: "test", csrf_token: "token" },
    get,
    post,
    streamSession,
  } as unknown as ControlApi;
  return { api, get, post, streamSession };
}

function sessionQuery(path: string): URLSearchParams {
  return new URL(path, "http://aibox.test").searchParams;
}

describe("ConfigPage", () => {
  function configFile(file: string, content: string) {
    return {
      file,
      exists: true,
      revision: `${file}-revision`,
      content_base64: btoa(content),
    };
  }

  it("renders row actions and protects Current and Applied Configs from bulk selection", async () => {
    const catalog = {
      named_configs: ["custom"],
      configs: [
        { name: "custom", state: "ready" },
        {
          name: "draft",
          state: "incomplete",
          detail: "Missing required file: auth.json. Use Repair to restore this Named Config.",
        },
        { name: "broken", state: "invalid", detail: "invalid permissions" },
      ],
      files: ["config.toml", "auth.json"],
      application: {
        last_application: {
          applied: "custom",
          applied_at: "2026-08-17T00:00:00Z",
        },
        drift: "dirty",
      },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((path: string, body: { file?: string }) => {
      if (path === "/_aibox/api/configs/reveal") {
        return Promise.resolve(configFile(body.file ?? "config.toml", "current content"));
      }
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    } as unknown as ControlApi;
    const user = userEvent.setup();

    render(<ConfigPage api={api} />);

    const name = await screen.findByText("custom");
    const drift = screen.getByText("Dirty");
    expect(screen.queryByText("Applied")).not.toBeInTheDocument();
    expect(name.parentElement).toHaveClass(styles.configRowTitle);
    expect(drift.parentElement).toBe(name.parentElement);
    expect(name.compareDocumentPosition(drift) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Agent: Codex" }).querySelector('[data-icon="codex"]'),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Propagate credentials" })).not.toBeInTheDocument();
    expect(screen.getByText("Current").closest("button")).toContainElement(
      document.querySelector('[data-icon="current-config"]'),
    );
    expect(document.querySelectorAll(`.${styles.configRowText} small`)).toHaveLength(0);
    expect(screen.queryByText("Native Config")).not.toBeInTheDocument();
    expect(within(screen.getByRole("button", { name: "custom" })).queryByRole("img")).toBeNull();
    const warningMarker = screen.getByRole("img", {
      name: /Config warning: Incomplete Config.*Missing required file: auth.json/,
    });
    const errorMarker = screen.getByRole("img", {
      name: /Config error: Invalid Config.*invalid permissions/,
    });
    expect(screen.getByRole("button", { name: "draft" })).toHaveAccessibleDescription(
      "Config warning: Incomplete Config. Missing required file: auth.json. Use Repair to restore this Named Config.",
    );
    expect(screen.getByRole("button", { name: "broken" })).toHaveAccessibleDescription(
      "Config error: Invalid Config. invalid permissions",
    );

    await user.hover(warningMarker);
    let tooltip = await screen.findByRole("tooltip");
    expect(tooltip).toHaveTextContent("Warning · Incomplete Config");
    expect(tooltip).toHaveTextContent("Missing required file: auth.json");
    await user.unhover(warningMarker);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    await user.hover(errorMarker);
    tooltip = await screen.findByRole("tooltip");
    expect(tooltip).toHaveTextContent("Error · Invalid Config");
    expect(tooltip).toHaveTextContent("invalid permissions");
    await user.unhover(errorMarker);
    const apply = screen.getByRole("button", { name: "Apply Named Config custom" });
    expect(apply).toBeEnabled();
    expect(apply).toHaveTextContent(/^Apply$/);
    expect(apply.querySelector("svg")).not.toBeInTheDocument();
    const repair = screen.getByRole("button", { name: "Repair Named Config draft" });
    expect(repair).toHaveTextContent(/^Repair$/);
    expect(repair.querySelector("svg")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Apply Named Config broken" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete Named Config broken" })).toBeInTheDocument();
    expect(await screen.findByRole("textbox", { name: "config.toml content" })).toHaveValue(
      "current content",
    );

    await user.click(screen.getByRole("button", { name: "Select Configs" }));
    const protectedCurrent = screen.getByRole("button", {
      name: "Current Config cannot be selected",
    });
    const protectedApplied = screen.getByRole("button", {
      name: "custom is Applied and cannot be selected",
    });
    expect(protectedCurrent).toBeDisabled();
    expect(protectedApplied).toBeDisabled();
    expect(protectedCurrent.parentElement).toHaveClass(styles.configRowProtected);
    expect(protectedApplied.parentElement).toHaveClass(styles.configRowProtected);
    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deselect draft" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Deselect broken" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(warningMarker).toBeInTheDocument();
    expect(errorMarker).toBeInTheDocument();
  });

  it.each([
    ["clean", true, "Clean"],
    ["dirty", false, "Dirty"],
    ["comparison-error", false, "Comparison error"],
  ] as const)(
    "keeps Apply visible and sets its disabled state for %s drift",
    async (drift, disabled, label) => {
      const catalog = {
        named_configs: ["custom"],
        configs: [{ name: "custom", state: "ready" }],
        files: ["config.toml", "auth.json"],
        application: {
          last_application: {
            applied: "custom",
            applied_at: "2026-08-17T00:00:00Z",
          },
          drift,
          detail: drift === "comparison-error" ? "could not compare Current Config" : undefined,
        },
        credential_propagation_available: false,
      } satisfies ConfigListData;
      const get = vi.fn((path: string) => {
        if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
        if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
        return Promise.reject(new Error(`Unexpected GET ${path}`));
      });
      const post = vi.fn((_path: string, body: { file: string }) =>
        Promise.resolve(configFile(body.file, "")),
      );
      const api = {
        bootstrap: { version: "test", csrf_token: "token" },
        get,
        post,
      } as unknown as ControlApi;

      render(<ConfigPage api={api} />);

      expect(await screen.findByText(label)).toBeInTheDocument();
      const apply = screen.getByRole("button", { name: "Apply Named Config custom" });
      if (disabled) expect(apply).toBeDisabled();
      else expect(apply).toBeEnabled();
      expect(screen.queryByText("Applied")).not.toBeInTheDocument();
    },
  );

  it("shows Source missing beside an incomplete applied Config and offers Repair", async () => {
    const catalog = {
      named_configs: [],
      configs: [{ name: "partial", state: "incomplete" }],
      files: ["config.toml", "auth.json"],
      application: {
        last_application: {
          applied: "partial",
          applied_at: "2026-08-17T00:00:00Z",
        },
        drift: "source-missing",
      },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((_path: string, body: { file: string }) =>
      Promise.resolve(configFile(body.file, "")),
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    } as unknown as ControlApi;

    render(<ConfigPage api={api} />);

    const name = await screen.findByText("partial");
    const drift = screen.getByText("Source missing");
    expect(drift.parentElement).toBe(name.parentElement);
    expect(screen.getByRole("button", { name: "Repair Named Config partial" })).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "Apply Named Config partial" }),
    ).not.toBeInTheDocument();
  });

  it("shows Propagate only for Host Codex with an eligible Current credential source", async () => {
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) {
        const query = new URL(path, "http://aibox.test").searchParams;
        const agent = query.get("agent");
        return Promise.resolve({
          named_configs: [],
          configs: [],
          files: agent === "claude" ? ["settings.json"] : ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: query.get("scope") === "host" && agent === "codex",
        } satisfies ConfigListData);
      }
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((_path: string, body: { file: string }) =>
      Promise.resolve(configFile(body.file, "")),
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    } as unknown as ControlApi;
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);

    await screen.findByRole("button", { name: "Tenant: default" });
    expect(screen.queryByRole("button", { name: "Propagate credentials" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    expect(
      screen.queryByRole("button", { name: "Select multiple tenants" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Host Tenant" }));
    const propagate = await screen.findByRole("button", { name: "Propagate credentials" });
    expect(propagate).toHaveTextContent(/^Propagate$/);
    expect(propagate.querySelector("svg")).not.toBeInTheDocument();
    expect(propagate).toHaveClass(styles.configPropagateAction);

    await user.click(screen.getByRole("button", { name: "Agent: Codex" }));
    expect(
      screen.queryByRole("button", { name: "Select multiple agents" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Claude" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Propagate credentials" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("creates a DNS-label Named Config and opens its detail", async () => {
    let configs: ConfigListData["configs"] = [];
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) {
        return Promise.resolve({
          named_configs: configs
            .filter((entry) => entry.state === "ready")
            .map((entry) => entry.name),
          configs: [...configs],
          files: ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        } satisfies ConfigListData);
      }
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((path: string, body: { config?: string; file?: string }) => {
      if (path === "/_aibox/api/configs/reveal") {
        return Promise.resolve(configFile(body.file ?? "config.toml", ""));
      }
      if (path === "/_aibox/api/configs/create") {
        configs = [{ name: body.config ?? "", state: "ready" }];
        return Promise.resolve({ created: body.config });
      }
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    } as unknown as ControlApi;
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);

    await user.click(await screen.findByRole("button", { name: "Create Named Config" }));
    const dialog = screen.getByRole("dialog", { name: "Create Named Config" });
    const input = within(dialog).getByRole("textbox", { name: "Named Config name" });
    await user.type(input, "Bad Name");
    expect(within(dialog).getByRole("button", { name: "Create" })).toBeDisabled();
    await user.clear(input);
    await user.type(input, "new-config");
    await user.keyboard("{Enter}");

    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/configs/create", {
        scope: "managed",
        tenant: "default",
        agent: "codex",
        config: "new-config",
      }),
    );
    expect(await screen.findByRole("button", { name: "new-config" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.queryByRole("dialog", { name: "Create Named Config" })).not.toBeInTheDocument();
  });

  it("reconciles surviving selections after a non-transactional batch deletion failure", async () => {
    let configs: ConfigListData["configs"] = [
      { name: "first", state: "ready" },
      { name: "second", state: "ready" },
    ];
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) {
        return Promise.resolve({
          named_configs: configs.map((entry) => entry.name),
          configs: [...configs],
          files: ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        } satisfies ConfigListData);
      }
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn((path: string, body: { file?: string }) => {
      if (path === "/_aibox/api/configs/reveal") {
        return Promise.resolve(configFile(body.file ?? "config.toml", ""));
      }
      if (path === "/_aibox/api/configs/delete") {
        configs = [{ name: "second", state: "ready" }];
        return Promise.reject(new Error("second Named Config could not be deleted"));
      }
      return Promise.reject(new Error(`Unexpected POST ${path}`));
    });
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    } as unknown as ControlApi;
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);

    await screen.findByRole("button", { name: "first" });
    await user.click(screen.getByRole("button", { name: "Select Configs" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Named Configs" }));
    const dialog = screen.getByRole("dialog", { name: "Delete selected Named Configs?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete selected" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "second Named Config could not be deleted",
    );
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deselect second" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.queryByRole("button", { name: "first" })).not.toBeInTheDocument();
  });

  it("guards file and Config switches when automatically revealed content is dirty", async () => {
    const catalog = {
      named_configs: ["other"],
      configs: [{ name: "other", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: { last_application: null, drift: "untracked" },
      credential_propagation_available: false,
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const post = vi.fn(
      (
        path: string,
        body: { file: string; current?: boolean; config?: string; content_base64?: string },
      ) => {
        if (path === "/_aibox/api/configs/reveal") {
          const owner = body.current ? "current" : body.config;
          return Promise.resolve(configFile(body.file, `${owner}:${body.file}`));
        }
        if (path === "/_aibox/api/configs/save") {
          return Promise.resolve({
            ...configFile(body.file, "saved"),
            content_base64: body.content_base64,
          });
        }
        return Promise.reject(new Error(`Unexpected POST ${path}`));
      },
    );
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post,
    } as unknown as ControlApi;
    const user = userEvent.setup();
    render(<ConfigPage api={api} />);

    const editor = await screen.findByRole("textbox", { name: "config.toml content" });
    expect(editor).toHaveValue("current:config.toml");
    await user.clear(editor);
    await user.type(editor, "changed main");
    await user.click(screen.getByRole("tab", { name: "auth.json" }));

    let dialog = screen.getByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("tab", { name: "config.toml" })).toHaveAttribute(
      "aria-selected",
      "true",
    );

    await user.click(screen.getByRole("tab", { name: "auth.json" }));
    dialog = screen.getByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(dialog).getByRole("button", { name: "Discard and continue" }));
    const authEditor = await screen.findByRole("textbox", { name: "auth.json content" });
    expect(authEditor).toHaveValue("current:auth.json");

    await user.clear(authEditor);
    await user.type(authEditor, "changed auth");
    await user.click(screen.getByRole("button", { name: "other" }));
    dialog = screen.getByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(dialog).getByRole("button", { name: "Save and continue" }));

    await waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        "/_aibox/api/configs/save",
        expect.objectContaining({ current: true, file: "auth.json" }),
      ),
    );
    expect(await screen.findByRole("textbox", { name: "auth.json content" })).toHaveValue(
      "other:auth.json",
    );
  });
});

describe("SessionPage", () => {
  it("defaults to compact single-select Tenant and Agent menus", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    const tenantTrigger = screen.getByRole("button", { name: "Tenant: default" });
    const agentTrigger = screen.getByRole("button", { name: "Agent: Codex" });
    expect(tenantTrigger).toHaveTextContent("default");
    expect(tenantTrigger).not.toHaveTextContent("Tenant:");
    expect(agentTrigger).toHaveTextContent("Codex");
    expect(agentTrigger).not.toHaveTextContent("Agent:");
    expect(agentTrigger.querySelector('[data-icon="codex"]')).toBeInTheDocument();
    expect(agentTrigger.querySelector('[data-icon="codex"]')?.parentElement).toHaveClass(
      styles.sessionFilterTriggerIcon,
    );
    expect(agentTrigger.querySelector(`.${styles.sessionFilterTriggerSummary}`)).toHaveTextContent(
      "Codex",
    );

    await user.click(tenantTrigger);
    const tenantMenu = screen.getByRole("dialog", { name: "Tenant" });
    expect(
      within(tenantMenu).getByRole("option", { name: "default (not created)" }),
    ).toHaveAttribute("aria-selected", "true");
    expect(within(tenantMenu).getByRole("option", { name: "Host Tenant" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    expect(
      within(tenantMenu).getByRole("button", { name: "Select multiple tenants" }),
    ).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(tenantTrigger).toHaveFocus();

    const session = await screen.findByRole("button", {
      name: "First prompt, Tenant default · Codex",
    });
    expect(session.querySelector('[data-icon="session-record"]')).toHaveClass("lucide-file-clock");
    const metadata = session.querySelector("small");
    expect(metadata).toHaveTextContent("default · Codex");
    expect(within(metadata!).getByText("2026-08-17 17:00:00").tagName).toBe("TIME");
    expect(metadata?.textContent).not.toContain("Codex · 2026");
    expect(session).not.toHaveTextContent("Tenant");
    expect(session).not.toHaveTextContent(firstSession.display_id);
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).toHaveTextContent("Refresh");
    expect(screen.getByRole("button", { name: "Select Sessions" })).toHaveTextContent("Select");
    await user.click(agentTrigger);
    const agentMenu = screen.getByRole("dialog", { name: "Agent" });
    const codexOption = within(agentMenu).getByRole("option", { name: "Codex" });
    const claudeOption = within(agentMenu).getByRole("option", { name: "Claude" });
    for (const option of [codexOption, claudeOption]) {
      expect(option).toHaveClass(styles.sessionFilterOptionSingle);
      expect(option.children[0]).toHaveClass(styles.sessionFilterOptionIcon);
      expect(option.children[1]).toHaveClass(styles.sessionFilterOptionLabel);
      expect(option.children[2]).toHaveClass(styles.sessionFilterOptionCheckSlot);
    }
    expect(codexOption.children[2].querySelector("svg")).toBeInTheDocument();
    expect(claudeOption.children[2]).toBeEmptyDOMElement();
    await user.click(claudeOption);

    expect(
      await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Agent: Claude" })).toHaveTextContent("Claude");
  });

  it("stages multiple values, cancels drafts, and can return to one value", async () => {
    const { api, get } = fakeApi({
      sessions: (path) =>
        sessionQuery(path).get("agent") === "claude" ? list([secondSession]) : list([firstSession]),
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    const agentTrigger = screen.getByRole("button", { name: "Agent: Codex" });
    await user.click(agentTrigger);
    let menu = screen.getByRole("dialog", { name: "Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple agents" }));
    const codexCheckbox = within(menu).getByRole("checkbox", { name: "Codex" });
    const claudeCheckbox = within(menu).getByRole("checkbox", { name: "Claude" });
    expect(codexCheckbox).toBeChecked();
    expect(codexCheckbox).toBeDisabled();
    expect(claudeCheckbox.closest("label")).toHaveClass(styles.sessionFilterOptionMultiple);
    expect(claudeCheckbox.closest("label")?.children[1]).toHaveClass(
      styles.sessionFilterOptionIcon,
    );
    expect(claudeCheckbox.closest("label")?.children[2]).toHaveClass(
      styles.sessionFilterOptionLabel,
    );
    await user.click(claudeCheckbox);
    expect(within(menu).getByRole("button", { name: "Apply" })).toBeEnabled();
    expect(get.mock.calls.some(([path]) => String(path).includes("agent=claude"))).toBe(false);
    await user.keyboard("{Escape}");
    expect(agentTrigger).toHaveFocus();
    expect(screen.getByRole("button", { name: "Agent: Codex" })).toBeInTheDocument();

    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(within(menu).getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Agent: Codex" })).toBeInTheDocument();

    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(document.body);
    expect(screen.queryByRole("dialog", { name: "Agent" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Agent: Codex" })).toBeInTheDocument();

    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(within(menu).getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(get).toHaveBeenCalledWith(
        expect.stringContaining("agent=claude"),
        expect.any(AbortSignal),
      ),
    );
    const multipleAgentTrigger = screen.getByRole("button", { name: "Agent: 2 agents" });
    expect(multipleAgentTrigger).toHaveTextContent("2 agents");
    expect(
      multipleAgentTrigger.querySelector(`.${styles.sessionFilterSummaryFull}`),
    ).toHaveTextContent("2 agents");
    expect(
      multipleAgentTrigger.querySelector(`.${styles.sessionFilterSummaryCompact}`),
    ).toHaveTextContent("2");

    await user.click(multipleAgentTrigger);
    menu = screen.getByRole("dialog", { name: "Agent" });
    expect(within(menu).getByRole("checkbox", { name: "Codex" })).toBeChecked();
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeChecked();
    expect(within(menu).getByRole("button", { name: "Apply" })).toBeDisabled();
    await user.click(within(menu).getByRole("checkbox", { name: "Codex" }));
    expect(within(menu).getByRole("checkbox", { name: "Codex" })).not.toBeChecked();
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeDisabled();
    expect(within(menu).getByRole("button", { name: "Choose one Agent" })).toBeInTheDocument();
    await user.click(within(menu).getByRole("button", { name: "Cancel" }));

    await user.click(screen.getByRole("button", { name: "Agent: 2 agents" }));
    menu = screen.getByRole("dialog", { name: "Agent" });
    await user.click(within(menu).getByRole("button", { name: "Choose one Agent" }));
    await user.click(within(menu).getByRole("button", { name: "Back to multiple agents" }));
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeChecked();
    await user.click(within(menu).getByRole("button", { name: "Choose one Agent" }));
    await user.click(within(menu).getByRole("option", { name: "Claude" }));

    expect(screen.getByRole("button", { name: "Agent: Claude" })).toHaveTextContent("Claude");
    expect(
      await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" }),
    ).toBeInTheDocument();
  });

  it("aborts a stale Session list request when the Coding Agent changes", async () => {
    const codexList = deferred<SessionListData>();
    let codexCalls = 0;
    let codexSignal: AbortSignal | undefined;
    const { api } = fakeApi({
      sessions: (path, signal) => {
        if (path.includes("agent=codex")) {
          codexCalls += 1;
          if (codexCalls > 1) return list([firstSession]);
          codexSignal = signal;
          signal?.addEventListener("abort", () =>
            codexList.reject(new DOMException("Aborted", "AbortError")),
          );
          return codexList.promise;
        }
        return list([secondSession]);
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await waitFor(() => expect(codexSignal).toBeDefined());
    await user.click(screen.getByRole("button", { name: "Agent: Codex" }));
    await user.click(screen.getByRole("option", { name: "Claude" }));

    expect(codexSignal?.aborted).toBe(true);
    expect(
      await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" }),
    ).toBeInTheDocument();
  });

  it("clears the manual refresh state when an Agent change replaces the request", async () => {
    const refresh = deferred<SessionListData>();
    let codexCalls = 0;
    let refreshSignal: AbortSignal | undefined;
    const { api } = fakeApi({
      sessions: (path, signal) => {
        if (path.includes("agent=claude")) return list([secondSession]);
        codexCalls += 1;
        if (codexCalls === 1) return list([firstSession]);
        if (codexCalls > 2) return list([firstSession]);
        refreshSignal = signal;
        signal?.addEventListener("abort", () =>
          refresh.reject(new DOMException("Aborted", "AbortError")),
        );
        return refresh.promise;
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Refresh Sessions" }));
    await waitFor(() => expect(refreshSignal).toBeDefined());
    await user.click(screen.getByRole("button", { name: "Agent: Codex" }));
    await user.click(screen.getByRole("option", { name: "Claude" }));

    expect(refreshSignal?.aborted).toBe(true);
    await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" });
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).toBeEnabled();
  });

  it("deletes one Session immediately, aborts its prompt stream, and restores list focus", async () => {
    let rows = [firstSession, secondSession];
    const deletion = deferred<{ deleted: number }>();
    let promptSignal: AbortSignal | undefined;
    const post = vi.fn(() => deletion.promise);
    const streamSession = vi.fn((_path: string, _onPrompt: unknown, signal?: AbortSignal) => {
      promptSignal = signal;
      return new Promise((_resolve, reject) => {
        signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")));
      });
    });
    const { api } = fakeApi({ sessions: () => list(rows), post, streamSession });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(promptSignal).toBeDefined();
    await user.click(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    );

    expect(promptSignal?.aborted).toBe(true);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Deleting Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).toBeDisabled();
    expect(post).toHaveBeenCalledWith("/_aibox/api/sessions/delete", {
      scope: "managed",
      tenant: "default",
      agent: "codex",
      ids: [firstSession.id],
      all: false,
      confirmation: "",
    });

    rows = [secondSession];
    act(() => deletion.resolve({ deleted: 1 }));

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "First prompt, Tenant default · Codex" }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByText("Select a Session")).toBeInTheDocument();
    expect(document.querySelector('[data-icon="session-empty"]')).toHaveClass("lucide-file-clock");
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "Delete Session 222222222222 from Tenant default · Codex",
        }),
      ).toHaveFocus(),
    );
  });

  it("selects the loaded snapshot and confirms deletion of only those explicit IDs", async () => {
    let rows = [firstSession, secondSession];
    const post = vi.fn().mockResolvedValue({ deleted: 2 });
    const { api } = fakeApi({ sessions: () => list(rows), post });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    expect(screen.queryByRole("button", { name: "Delete all" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const count = screen.getByText("0 selected");
    const selectAll = screen.getByRole("button", { name: "Select all" });
    const deleteSelected = screen.getByRole("button", { name: "Delete selected Sessions" });
    expect(cancel.compareDocumentPosition(count) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      count.compareDocumentPosition(selectAll) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      selectAll.compareDocumentPosition(deleteSelected) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    await user.click(cancel);
    expect(screen.getByRole("button", { name: "Select Sessions" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect First prompt, Tenant default · Codex" }),
    ).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));

    const dialog = screen.getByRole("dialog", { name: "Delete 2 selected Sessions?" });
    expect(dialog).toHaveTextContent("Sources: Tenant default · Codex (2)");
    rows = [thirdSession];
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));

    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/_aibox/api/sessions/delete", {
        scope: "managed",
        tenant: "default",
        agent: "codex",
        ids: [firstSession.id, secondSession.id],
        all: false,
        confirmation: "",
      }),
    );
    expect(
      await screen.findByRole("button", { name: "New prompt, Tenant default · Codex" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Sessions" })).toBeInTheDocument();
  });

  it("reconciles surviving selections after a non-transactional batch failure", async () => {
    let rows = [firstSession, secondSession];
    const post = vi.fn().mockImplementation(() => {
      rows = [secondSession];
      return Promise.reject(new Error("second Transcript could not be deleted"));
    });
    const { api } = fakeApi({ sessions: () => list(rows), post });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("second Transcript could not be deleted");
    expect(within(alert).queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect Second prompt, Tenant default · Codex" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("button", { name: "Select Sessions" })).not.toBeInTheDocument();
  });

  it("disables deletion for an incomplete view but not for Transcript content warnings", async () => {
    const warnedSession = {
      ...firstSession,
      warnings: ["skipped 1 malformed JSONL record(s)"],
    };
    const incomplete = fakeApi({
      sessions: () => list([warnedSession], ["walk session directory: permission denied"]),
    });
    const firstRender = render(<SessionPage api={incomplete.api} />);

    expect(await screen.findByRole("button", { name: "Select Sessions" })).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeDisabled();

    firstRender.unmount();
    const readable = fakeApi({ sessions: () => list([warnedSession]) });
    render(<SessionPage api={readable.api} />);

    expect(await screen.findByRole("button", { name: "Select Sessions" })).toBeEnabled();
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeEnabled();
  });

  it("names the real Host Home in the selected deletion confirmation", async () => {
    const { api, post } = fakeApi();
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    await user.click(screen.getByRole("option", { name: "Host Tenant" }));
    const hostSession = await screen.findByRole("button", {
      name: "First prompt, Host Tenant · Codex",
    });
    expect(hostSession.querySelector("small")).toHaveTextContent("Host Tenant · Codex");
    expect(within(hostSession).getByText("2026-08-17 17:00:00").tagName).toBe("TIME");
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Sources: Host Tenant · Codex (2)");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(post).not.toHaveBeenCalled();
  });

  it("aggregates every selected Tenant and Coding Agent with stable source identities", async () => {
    const streamSession = vi.fn().mockResolvedValue({ id: firstSession.id, warnings: [] });
    const { api, get } = fakeApi({
      sessions: (path) => {
        const query = sessionQuery(path);
        const tenant = query.get("tenant") ?? "host";
        const agent = query.get("agent") ?? "codex";
        const offsets: Record<string, string> = {
          "default:codex": "2026-08-17T09:00:00Z",
          "default:claude": "2026-08-17T07:00:00Z",
          "work:codex": "2026-08-17T08:00:00Z",
          "work:claude": "2026-08-17T10:00:00Z",
        };
        return list([
          {
            ...firstSession,
            start_ts: offsets[`${tenant}:${agent}`],
            title: `${tenant} ${agent}`,
          },
        ]);
      },
      streamSession,
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "default codex, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    let filterMenu = screen.getByRole("dialog", { name: "Tenant" });
    await user.click(within(filterMenu).getByRole("button", { name: "Select multiple tenants" }));
    await user.click(within(filterMenu).getByRole("checkbox", { name: "work" }));
    await user.click(within(filterMenu).getByRole("button", { name: "Apply" }));
    await user.click(screen.getByRole("button", { name: "Agent: Codex" }));
    filterMenu = screen.getByRole("dialog", { name: "Agent" });
    await user.click(within(filterMenu).getByRole("button", { name: "Select multiple agents" }));
    await user.click(within(filterMenu).getByRole("checkbox", { name: "Claude" }));
    await user.click(within(filterMenu).getByRole("button", { name: "Apply" }));

    const newest = await screen.findByRole("button", {
      name: "work claude, Tenant work · Claude",
    });
    expect(
      screen.getByRole("button", { name: "default codex, Tenant default · Codex" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "work codex, Tenant work · Codex" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "default claude, Tenant default · Claude" }),
    ).toBeInTheDocument();
    expect(newest.querySelector("small")).toHaveTextContent("work · Claude");
    expect(within(newest).getByText("2026-08-17 18:00:00").tagName).toBe("TIME");
    expect(newest).not.toHaveTextContent(firstSession.display_id);
    expect(get).toHaveBeenCalledWith(
      expect.stringContaining("tenant=work"),
      expect.any(AbortSignal),
    );
    expect(get).toHaveBeenCalledWith(
      expect.stringContaining("agent=claude"),
      expect.any(AbortSignal),
    );

    await user.click(newest);
    expect(streamSession).toHaveBeenCalledWith(
      expect.stringMatching(/tenant=work.*agent=claude|agent=claude.*tenant=work/),
      expect.any(Function),
      expect.any(AbortSignal),
    );
    expect(
      screen
        .getAllByText(/Tenant work · Claude ·/)
        .some((element) => element.textContent?.includes(firstSession.id)),
    ).toBe(true);
  });

  it("keeps readable sources but disables deletion when one source cannot be listed", async () => {
    const { api } = fakeApi({
      sessions: (path) => {
        if (sessionQuery(path).get("tenant") === "work") throw new Error("permission denied");
        return list([firstSession]);
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    const filterMenu = screen.getByRole("dialog", { name: "Tenant" });
    await user.click(within(filterMenu).getByRole("button", { name: "Select multiple tenants" }));
    await user.click(within(filterMenu).getByRole("checkbox", { name: "work" }));
    await user.click(within(filterMenu).getByRole("button", { name: "Apply" }));

    expect(await screen.findByText("Tenant work · Codex: permission denied")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "First prompt, Tenant default · Codex" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Sessions" })).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).toBeDisabled();
  });

  it("deletes cross-source selections serially and preserves failed survivors", async () => {
    let defaultRows = [firstSession];
    const workRows = [secondSession];
    const defaultDeletion = deferred<{ deleted: number }>();
    const post = vi.fn((_path: string, body: { tenant?: string }) => {
      if (body.tenant === "default") {
        return defaultDeletion.promise.then((result) => {
          defaultRows = [];
          return result;
        });
      }
      return Promise.reject(new Error("work Transcript could not be deleted"));
    });
    const { api } = fakeApi({
      sessions: (path) =>
        list(sessionQuery(path).get("tenant") === "work" ? workRows : defaultRows),
      post,
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    await user.click(screen.getByRole("button", { name: "Tenant: default" }));
    const filterMenu = screen.getByRole("dialog", { name: "Tenant" });
    await user.click(within(filterMenu).getByRole("button", { name: "Select multiple tenants" }));
    await user.click(within(filterMenu).getByRole("checkbox", { name: "work" }));
    await user.click(within(filterMenu).getByRole("button", { name: "Apply" }));
    await screen.findByRole("button", { name: "Second prompt, Tenant work · Codex" });
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));

    const dialog = screen.getByRole("dialog", { name: "Delete 2 selected Sessions?" });
    expect(dialog).toHaveTextContent("Tenant default · Codex (1)");
    expect(dialog).toHaveTextContent("Tenant work · Codex (1)");
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    expect(post).toHaveBeenCalledTimes(1);

    act(() => defaultDeletion.resolve({ deleted: 1 }));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "work Transcript could not be deleted",
    );
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Deselect Second prompt, Tenant work · Codex" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.queryByRole("button", { name: "Select First prompt, Tenant default · Codex" }),
    ).not.toBeInTheDocument();
    expect(post.mock.calls[0][1]).toMatchObject({ tenant: "default", agent: "codex" });
    expect(post.mock.calls[1][1]).toMatchObject({ tenant: "work", agent: "codex" });
  });

  it("uses two-level copy for list, detail, and empty Transcript states", async () => {
    const empty = fakeApi({ sessions: () => list([]) });
    const firstRender = render(<SessionPage api={empty.api} />);

    expect(await screen.findByText("No Sessions found")).toBeInTheDocument();
    expect(
      screen.getByText("No Sessions were found for the selected Tenants and Coding Agents."),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Select a Session" })).toBeInTheDocument();
    expect(
      screen.getByText("Choose a Session to inspect its prompts and Transcript warnings."),
    ).toBeInTheDocument();
    firstRender.unmount();

    const readable = fakeApi({ sessions: () => list([firstSession]) });
    const user = userEvent.setup();
    render(<SessionPage api={readable.api} />);
    await user.click(
      await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" }),
    );
    expect(await screen.findByRole("heading", { name: "No typed prompts" })).toBeInTheDocument();
    expect(
      screen.getByText("This Session's Transcript contains no supported typed user prompts."),
    ).toBeInTheDocument();
  });
});
