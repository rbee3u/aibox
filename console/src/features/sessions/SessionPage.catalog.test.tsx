import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import type { SessionListData } from "@/api/sessions";
import { deferred } from "@/test/deferred";
import {
  SessionPage,
  firstSession,
  secondSession,
  list,
  fakeApi,
} from "@/features/sessions/testSupport";

afterEach(() => {
  window.history.replaceState(null, "", "/");
});
describe("SessionPage", () => {
  it("defaults to compact single-select Tenant and Agent menus", async () => {
    const { api } = fakeApi();
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    const tenantTrigger = await screen.findByRole("button", { name: "Tenant: default" });
    const agentTrigger = screen.getByRole("button", { name: "Coding Agent: Codex" });
    expect(tenantTrigger).toHaveTextContent("default");
    expect(tenantTrigger).not.toHaveTextContent("Tenant:");
    expect(agentTrigger).toHaveTextContent("Codex");
    expect(agentTrigger).not.toHaveTextContent("Coding Agent:");
    const agentIcon = agentTrigger.querySelector<HTMLElement>('[data-icon="openai"]');
    expect(agentIcon).toBeInTheDocument();
    expect(agentIcon?.style.getPropertyValue("--brand-icon-size")).toBe("14px");
    await user.click(tenantTrigger);
    const tenantMenu = screen.getByRole("dialog", { name: "Tenant" });
    expect(within(tenantMenu).getByRole("option", { name: "default" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
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
    expect(session.querySelector('[data-icon="session-record"]')).toHaveClass(
      "lucide-messages-square",
    );
    expect(within(session).getByText("First prompt", { selector: "strong" })).toBeInTheDocument();
    const metadata = session.querySelector("small");
    expect(metadata).toHaveTextContent("default Codex");
    const sessionTime = within(metadata!).getByText("2026-08-17 17:00:00");
    expect(sessionTime.tagName).toBe("TIME");
    expect(sessionTime).toHaveAttribute("datetime", firstSession.start_ts);
    expect(metadata?.textContent).not.toContain("Codex 2026");
    expect(session).not.toHaveTextContent("Tenant");
    expect(session).not.toHaveTextContent(firstSession.display_id);
    expect(
      screen.getByRole("button", {
        name: "Delete Session 111111111111 from Tenant default · Codex",
      }),
    ).not.toHaveAttribute("title");
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).not.toHaveAttribute("title");
    expect(screen.getByRole("button", { name: "Select Sessions" })).toHaveTextContent("Select");
    await user.click(agentTrigger);
    const agentMenu = screen.getByRole("dialog", { name: "Coding Agent" });
    const codexOption = within(agentMenu).getByRole("option", { name: "Codex" });
    const claudeOption = within(agentMenu).getByRole("option", { name: "Claude" });
    expect(codexOption).toHaveAttribute("aria-selected", "true");
    expect(claudeOption).toHaveAttribute("aria-selected", "false");
    await user.click(claudeOption);
    expect(
      await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Coding Agent: Claude" })).toHaveTextContent(
      "Claude",
    );
  });
  it("reports a missing Managed Tenant in the Session selector", async () => {
    window.history.replaceState(
      null,
      "",
      "/_aibox/ui/sessions?tenant=managed%3Amissing&agent=codex",
    );
    const { api } = fakeApi({ sessions: () => list([]) });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    const tenantTrigger = await screen.findByRole("button", { name: "Tenant: Not found" });
    expect(
      screen.getByText("No Sessions were found for the selected Tenants and Coding Agents."),
    ).toBeInTheDocument();
    await user.click(tenantTrigger);
    const tenantMenu = screen.getByRole("dialog", { name: "Tenant" });
    for (const name of ["Host Tenant", "default", "work"]) {
      expect(within(tenantMenu).getByRole("option", { name })).toHaveAttribute(
        "aria-selected",
        "false",
      );
    }
  });
  it("keeps a complete long Session title in the two-line summary", async () => {
    const title =
      "A deliberately long Session title that remains available after its visual two-line clamp";
    const longSession = { ...firstSession, title };
    const { api } = fakeApi({ sessions: () => list([longSession]) });
    render(<SessionPage api={api} />);
    const session = await screen.findByRole("button", {
      name: `${title}, Tenant default · Codex`,
    });
    const titleElement = within(session).getByTitle(title);
    expect(titleElement.tagName).toBe("STRONG");
    expect(titleElement).toHaveTextContent(title);
  });
  it("stages multiple values, cancels drafts, and can return to one value", async () => {
    const { api, listSessions } = fakeApi({
      sessions: (_tenant, agent) =>
        agent === "claude" ? list([secondSession]) : list([firstSession]),
    });
    const user = userEvent.setup();
    render(<SessionPage api={api} />);
    await screen.findByRole("button", { name: "First prompt, Tenant default · Codex" });
    const agentTrigger = screen.getByRole("button", { name: "Coding Agent: Codex" });
    await user.click(agentTrigger);
    let menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple Coding Agents" }));
    const codexCheckbox = within(menu).getByRole("checkbox", { name: "Codex" });
    const claudeCheckbox = within(menu).getByRole("checkbox", { name: "Claude" });
    expect(codexCheckbox).toBeChecked();
    expect(codexCheckbox).toBeDisabled();
    expect(claudeCheckbox.closest("label")).toHaveAttribute("title", "Claude");
    await user.click(claudeCheckbox);
    expect(within(menu).getByRole("button", { name: "Apply" })).toBeEnabled();
    expect(listSessions.mock.calls.some(([, agent]) => agent === "claude")).toBe(false);
    await user.keyboard("{Escape}");
    expect(agentTrigger).toHaveFocus();
    expect(screen.getByRole("button", { name: "Coding Agent: Codex" })).toBeInTheDocument();
    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple Coding Agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(within(menu).getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Coding Agent: Codex" })).toBeInTheDocument();
    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple Coding Agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(document.body);
    expect(screen.queryByRole("dialog", { name: "Coding Agent" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Coding Agent: Codex" })).toBeInTheDocument();
    await user.click(agentTrigger);
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Select multiple Coding Agents" }));
    await user.click(within(menu).getByRole("checkbox", { name: "Claude" }));
    await user.click(within(menu).getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(listSessions).toHaveBeenCalledWith(
        expect.any(Object),
        "claude",
        expect.any(AbortSignal),
      ),
    );
    const multipleAgentTrigger = screen.getByRole("button", {
      name: "Coding Agent: 2 Coding Agents",
    });
    expect(multipleAgentTrigger).toHaveTextContent("2 Coding Agents");
    await user.click(multipleAgentTrigger);
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    expect(within(menu).getByRole("checkbox", { name: "Codex" })).toBeChecked();
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeChecked();
    expect(within(menu).getByRole("button", { name: "Apply" })).toBeDisabled();
    await user.click(within(menu).getByRole("checkbox", { name: "Codex" }));
    expect(within(menu).getByRole("checkbox", { name: "Codex" })).not.toBeChecked();
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeDisabled();
    expect(
      within(menu).getByRole("button", { name: "Choose one Coding Agent" }),
    ).toBeInTheDocument();
    await user.click(within(menu).getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Coding Agent: 2 Coding Agents" }));
    menu = screen.getByRole("dialog", { name: "Coding Agent" });
    await user.click(within(menu).getByRole("button", { name: "Choose one Coding Agent" }));
    await user.click(within(menu).getByRole("button", { name: "Back to multiple Coding Agents" }));
    expect(within(menu).getByRole("checkbox", { name: "Claude" })).toBeChecked();
    await user.click(within(menu).getByRole("button", { name: "Choose one Coding Agent" }));
    await user.click(within(menu).getByRole("option", { name: "Claude" }));
    expect(screen.getByRole("button", { name: "Coding Agent: Claude" })).toHaveTextContent(
      "Claude",
    );
    expect(
      await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" }),
    ).toBeInTheDocument();
  });
  it("aborts a stale Session list request when the Coding Agent changes", async () => {
    const codexList = deferred<SessionListData>();
    let codexCalls = 0;
    let codexSignal: AbortSignal | undefined;
    const { api } = fakeApi({
      sessions: (_tenant, agent, signal) => {
        if (agent === "codex") {
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
    await user.click(screen.getByRole("button", { name: "Coding Agent: Codex" }));
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
      sessions: (_tenant, agent, signal) => {
        if (agent === "claude") return list([secondSession]);
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
    await user.click(screen.getByRole("button", { name: "Coding Agent: Codex" }));
    await user.click(screen.getByRole("option", { name: "Claude" }));
    expect(refreshSignal?.aborted).toBe(true);
    await screen.findByRole("button", { name: "Second prompt, Tenant default · Claude" });
    expect(screen.getByRole("button", { name: "Refresh Sessions" })).toBeEnabled();
  });
});
