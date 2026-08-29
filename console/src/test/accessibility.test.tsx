import axe from "axe-core";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConfigPage } from "@/features/configs/ConfigPage";
import { SessionPage } from "@/features/sessions/SessionPage";
import { TenantPage } from "@/features/tenants/TenantPage";
import { OverviewPage } from "@/features/overview/OverviewPage";
import { RequestsPage } from "@/features/requests/RequestsPage";
import { composeControlApi } from "@/api/connect";
import type { OverviewData, TopologyData } from "@/api/overview";
import { ControlApi } from "@/api/transport";
import { materializeControlApi } from "@/test/managementTestSupport";
import { requestsApiFake } from "@/features/requests/testFixtures";
import { applyThemePreference } from "@/app/theme/usePersistentTheme";

type Module = "overview" | "tenants" | "configs" | "sessions" | "requests";
type Theme = "light" | "dark";

const overview = {
  service: {
    version: "1.2.3",
    listen: "127.0.0.1:9923",
    uptime_seconds: 60,
    aibox_root: "/tmp/aibox",
  },
  docker: { status: "available", error: null },
  runtime_image: {
    reference: "aibox:latest",
    status: "built",
    id: "sha256:1234567890abcdef",
    created_at: "2026-08-18T12:00:00Z",
    size_bytes: 1024,
    detail: null,
  },
  managed_tenants: 1,
  host_available: true,
  requests: { total: 2, active: 0, warning: 0, error: 0, bytes: 1024 },
} satisfies OverviewData;

const topology = { tenants: [] } satisfies TopologyData;
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
    name: "default",
    display_name: "default",
    home: "/tmp/aibox/tenants/default",
    exists: true,
  },
] as const;

function controlApi(): ControlApi {
  return materializeControlApi({
    bootstrap: { version: "1.2.3", csrf_token: "token" },
    get: vi.fn((path: string) => {
      if (path === "/_aibox/api/overview") return Promise.resolve(overview);
      if (path === "/_aibox/api/topology") return Promise.resolve(topology);
      if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
      if (path.startsWith("/_aibox/api/components?")) {
        return Promise.resolve([
          {
            kind: "codex-statusline",
            supports_version: false,
            status: "installed",
            version: null,
            error: null,
          },
        ]);
      }
      if (path.startsWith("/_aibox/api/configs?")) {
        return Promise.resolve({
          configs: [],
          files: ["config.toml", "auth.json"],
          application: { last_application: null, drift: "untracked" },
          credential_propagation_available: false,
        });
      }
      if (path.startsWith("/_aibox/api/sessions?")) {
        return Promise.resolve({ sessions: [], warnings: [], partial: false });
      }
      return Promise.reject(new Error(`Unexpected Control API request: ${path}`));
    }),
    post: vi.fn((path: string) => {
      if (path === "/_aibox/api/configs/reveal") {
        return Promise.resolve({
          file: "config.toml",
          exists: false,
          revision: "missing",
          content_base64: "",
        });
      }
      return Promise.reject(new Error(`Unexpected Control API mutation: ${path}`));
    }),
    streamSessionDetail: vi.fn(),
  });
}

async function renderModule(module: Module) {
  const api = composeControlApi(controlApi());
  switch (module) {
    case "overview": {
      const result = render(
        <OverviewPage
          api={api.overview}
          operation={null}
          onNavigate={() => undefined}
          onOperation={() => undefined}
        />,
      );
      await screen.findByRole("region", { name: "Service status" });
      return result;
    }
    case "tenants": {
      const result = render(
        <TenantPage api={api.tenants} search={window.location.search} onLocationChange={vi.fn()} />,
      );
      await screen.findByRole("button", { name: /^default, Managed Tenant/ });
      return result;
    }
    case "configs": {
      const result = render(
        <ConfigPage api={api.configs} search={window.location.search} onLocationChange={vi.fn()} />,
      );
      await screen.findByRole("button", { name: "Tenant: default" });
      return result;
    }
    case "sessions": {
      const result = render(
        <SessionPage
          api={api.sessions}
          search={window.location.search}
          onLocationChange={vi.fn()}
        />,
      );
      await screen.findByRole("button", { name: "Tenant: default" });
      return result;
    }
    case "requests": {
      const result = render(
        <RequestsPage
          api={requestsApiFake()}
          search={window.location.search}
          onLocationChange={vi.fn()}
        />,
      );
      await screen.findByRole("button", { name: "POST api.example.test/v1/responses" });
      return result;
    }
  }
}

afterEach(() => {
  window.history.replaceState(null, "", "/");
  document.documentElement.removeAttribute("data-theme");
});

describe("Console accessibility gate", () => {
  for (const theme of ["light", "dark"] satisfies Theme[]) {
    for (const module of [
      "overview",
      "tenants",
      "configs",
      "sessions",
      "requests",
    ] satisfies Module[]) {
      it(`${module} has no serious or critical axe violations in ${theme} theme`, async () => {
        applyThemePreference(theme);
        const { container } = await renderModule(module);
        const result = await axe.run(container, {
          runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] },
        });
        const blocking = result.violations.filter(
          (violation) => violation.impact === "serious" || violation.impact === "critical",
        );
        expect(
          blocking.map((violation) => ({
            id: violation.id,
            help: violation.help,
            targets: violation.nodes.map((node) => node.target),
          })),
        ).toEqual([]);
      });
    }
  }
});
