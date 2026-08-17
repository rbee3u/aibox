import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ControlApi } from "./controlApi";
import type { ConfigListData, SessionListData, SessionRow, TenantRow } from "./controlApi";
import { ConfigPage, SessionPage } from "./ManagementPages";
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
    display_name: "Host",
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

describe("ConfigPage", () => {
  it("renders the applied Named Config from Last Application", async () => {
    const catalog = {
      named_configs: ["custom"],
      configs: [{ name: "custom", state: "ready" }],
      files: ["config.toml", "auth.json"],
      application: {
        last_application: {
          applied: "custom",
          applied_at: "2026-08-17T00:00:00Z",
        },
        drift: "clean",
      },
    } satisfies ConfigListData;
    const get = vi.fn((path: string) => {
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/configs?")) return Promise.resolve(catalog);
      return Promise.reject(new Error(`Unexpected GET ${path}`));
    });
    const api = {
      bootstrap: { version: "test", csrf_token: "token" },
      get,
      post: vi.fn(),
    } as unknown as ControlApi;

    render(<ConfigPage api={api} />);

    expect(await screen.findByText("custom · clean")).toBeInTheDocument();
  });
});

describe("SessionPage", () => {
  it("shows an explicit missing default and switches Coding Agent with icon controls", async () => {
    const { api, get } = fakeApi();
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    const tenant = screen.getByRole("combobox", { name: "Tenant" });
    expect(tenant).toHaveValue("managed:default");
    expect(
      within(tenant).getByRole("option", { name: "default (not created)" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Codex" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Claude" })).toHaveAttribute("aria-pressed", "false");

    await screen.findByRole("button", { name: "First prompt" });
    await user.click(screen.getByRole("button", { name: "Claude" }));

    await waitFor(() =>
      expect(get).toHaveBeenCalledWith(
        expect.stringContaining("agent=claude"),
        expect.any(AbortSignal),
      ),
    );
    expect(screen.getByRole("button", { name: "Claude" })).toHaveAttribute("aria-pressed", "true");
  });

  it("aborts a stale Session list request when the Coding Agent changes", async () => {
    const codexList = deferred<SessionListData>();
    let codexSignal: AbortSignal | undefined;
    const { api } = fakeApi({
      sessions: (path, signal) => {
        if (path.includes("agent=codex")) {
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
    await user.click(screen.getByRole("button", { name: "Claude" }));

    expect(codexSignal?.aborted).toBe(true);
    expect(await screen.findByRole("button", { name: "Second prompt" })).toBeInTheDocument();
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
        refreshSignal = signal;
        signal?.addEventListener("abort", () =>
          refresh.reject(new DOMException("Aborted", "AbortError")),
        );
        return refresh.promise;
      },
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "First prompt" });
    await user.click(screen.getByRole("button", { name: "Refresh Sessions" }));
    await waitFor(() => expect(refreshSignal).toBeDefined());
    await user.click(screen.getByRole("button", { name: "Claude" }));

    expect(refreshSignal?.aborted).toBe(true);
    await screen.findByRole("button", { name: "Second prompt" });
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

    await user.click(await screen.findByRole("button", { name: "First prompt" }));
    expect(promptSignal).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Delete Session 111111111111" }));

    expect(promptSignal?.aborted).toBe(true);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deleting Session 111111111111" })).toBeDisabled();
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
      expect(screen.queryByRole("button", { name: "First prompt" })).not.toBeInTheDocument(),
    );
    expect(screen.getByText("Select a Session")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Delete Session 222222222222" })).toHaveFocus(),
    );
  });

  it("selects the loaded snapshot and confirms deletion of only those explicit IDs", async () => {
    let rows = [firstSession, secondSession];
    const post = vi.fn().mockResolvedValue({ deleted: 2 });
    const { api } = fakeApi({ sessions: () => list(rows), post });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("button", { name: "First prompt" });
    expect(screen.queryByRole("button", { name: "Delete all" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deselect First prompt" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));

    const dialog = screen.getByRole("dialog", { name: "Delete 2 selected Sessions?" });
    expect(dialog).toHaveTextContent(
      "permanently deletes the Transcripts for the selected Sessions in Tenant default for Codex",
    );
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
    expect(await screen.findByRole("button", { name: "New prompt" })).toBeInTheDocument();
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

    await screen.findByRole("button", { name: "First prompt" });
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));
    await user.click(screen.getByRole("button", { name: "Delete permanently" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("second Transcript could not be deleted");
    expect(within(alert).queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deselect Second prompt" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
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
    expect(screen.getByRole("button", { name: "Delete Session 111111111111" })).toBeDisabled();

    firstRender.unmount();
    const readable = fakeApi({ sessions: () => list([warnedSession]) });
    render(<SessionPage api={readable.api} />);

    expect(await screen.findByRole("button", { name: "Select Sessions" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Delete Session 111111111111" })).toBeEnabled();
  });

  it("names the real Host Home in the selected deletion confirmation", async () => {
    const { api, post } = fakeApi();
    const user = userEvent.setup();
    render(<SessionPage api={api} />);

    await screen.findByRole("option", { name: "Host" });
    await user.selectOptions(screen.getByRole("combobox", { name: "Tenant" }), "host");
    await screen.findByRole("button", { name: "First prompt" });
    await user.click(screen.getByRole("button", { name: "Select Sessions" }));
    await user.click(screen.getByRole("button", { name: "Select all" }));
    await user.click(screen.getByRole("button", { name: "Delete selected Sessions" }));

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("real Host Home for Codex");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(post).not.toHaveBeenCalled();
  });
});
