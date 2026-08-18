import { Box, Menu } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRequestApi } from "./api";
import { ConfigPage, OperationPanel, SessionPage, TenantPage } from "./ManagementPages";
import { OverviewPage, type ConsoleNavigate } from "./OverviewPage";
import { RequestsPage } from "./RequestsPage";
import { ControlApi } from "./controlApi";
import type { Operation } from "./controlApi";
import { readPreference, storePreference } from "./preferences";
import { SidebarUtilities } from "./SidebarUtilities";
import { usePersistentTheme } from "./usePersistentTheme";
import { moduleIcons, type ModuleId } from "./moduleIcons";
import styles from "./App.module.css";

const modules = [
  { id: "overview", label: "Overview", detail: "Current status", icon: moduleIcons.overview },
  { id: "tenants", label: "Tenants", detail: "Identity & Components", icon: moduleIcons.tenants },
  { id: "configs", label: "Configs", detail: "Native configuration", icon: moduleIcons.configs },
  { id: "sessions", label: "Sessions", detail: "Agent transcripts", icon: moduleIcons.sessions },
  {
    id: "requests",
    label: "Requests",
    detail: "Inspect your LLM requests",
    icon: moduleIcons.requests,
  },
] as const;

const SIDEBAR_COLLAPSED_KEY = "aibox-console-sidebar-collapsed";

function moduleFromPath(): ModuleId {
  const value = window.location.pathname.split("/").filter(Boolean).at(-1);
  return modules.some((module) => module.id === value) ? (value as ModuleId) : "overview";
}

export function App() {
  const [api, setApi] = useState<ControlApi | null>(null);
  const [active, setActive] = useState<ModuleId>(moduleFromPath);
  const [collapsed, setCollapsed] = useState(
    () => readPreference(SIDEBAR_COLLAPSED_KEY) === "true",
  );
  const [mobileOpen, setMobileOpen] = useState(false);
  const [theme, setTheme] = usePersistentTheme();
  const [operation, setOperation] = useState<Operation | null>(null);
  const [operationDismissed, setOperationDismissed] = useState<string | null>(null);
  const [startupError, setStartupError] = useState<string | null>(null);
  const [locationVersion, setLocationVersion] = useState(0);
  const configDirty = useRef(false);
  const acceptedLocation = useRef(currentLocation());

  useEffect(() => {
    void ControlApi.connect()
      .then((client) => {
        setApi(client);
        return client.get<{ operation: Operation | null }>("/_aibox/api/operations/current");
      })
      .then((value) => setOperation(value.operation))
      .catch((cause: unknown) =>
        setStartupError(cause instanceof Error ? cause.message : String(cause)),
      );
  }, []);

  useEffect(() => {
    if (!api) return;
    const source = new EventSource("/_aibox/api/operations/events");
    source.addEventListener("operation", (event) => {
      const value = JSON.parse((event as MessageEvent<string>).data) as {
        operation: Operation | null;
        gap: boolean;
      };
      setOperation((current) => mergeOperation(current, value.operation, value.gap));
      if (value.operation?.state === "running") setOperationDismissed(null);
    });
    return () => source.close();
  }, [api]);

  useEffect(() => {
    const onPopState = () => {
      const next = currentLocation();
      if (configDirty.current && !confirmDiscardedConfig()) {
        window.history.pushState(null, "", acceptedLocation.current);
        return;
      }
      acceptedLocation.current = next;
      setActive(moduleFromPath());
      setLocationVersion((value) => value + 1);
    };
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  useEffect(() => {
    const preventDirtyUnload = (event: BeforeUnloadEvent) => {
      if (!configDirty.current) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", preventDirtyUnload);
    return () => window.removeEventListener("beforeunload", preventDirtyUnload);
  }, []);

  useEffect(() => {
    storePreference(SIDEBAR_COLLAPSED_KEY, String(collapsed));
  }, [collapsed]);

  const requestApi = useMemo(
    () => (api ? createRequestApi(fetch, api.bootstrap.csrf_token) : null),
    [api],
  );
  const activeModule = modules.find((module) => module.id === active)!;

  const commitLocation = useCallback(
    (module: ModuleId, query?: URLSearchParams, replace = false) => {
      const suffix = query?.toString();
      const next = `/_aibox/ui/${module}${suffix ? `?${suffix}` : ""}`;
      window.history[replace ? "replaceState" : "pushState"](null, "", next);
      acceptedLocation.current = next;
    },
    [],
  );

  const navigate: ConsoleNavigate = (module, query) => {
    if (configDirty.current && !confirmDiscardedConfig()) return;
    commitLocation(module, query);
    setActive(module);
    setMobileOpen(false);
  };

  const updatePageLocation = useCallback(
    (module: ModuleId, query: URLSearchParams, replace = false) => {
      commitLocation(module, query, replace);
    },
    [commitLocation],
  );

  const recordConfigDirty = useCallback((dirty: boolean) => {
    configDirty.current = dirty;
  }, []);

  function recordOperation(value: Operation) {
    setOperation(value);
    setOperationDismissed(null);
  }

  return (
    <div className={`${styles.app} ${collapsed ? styles.collapsed : ""}`}>
      <aside
        className={`${styles.sidebar} ${mobileOpen ? styles.mobileOpen : ""}`}
        aria-label="Console navigation"
      >
        <div className={styles.brand}>
          <span className={styles.mark}>
            <Box size={23} strokeWidth={2.2} />
          </span>
          <strong>AIBox</strong>
        </div>
        <nav className={styles.moduleNav} aria-label="Modules">
          {modules.map((module) => {
            const Icon = module.icon;
            return (
              <button
                type="button"
                key={module.id}
                aria-current={active === module.id ? "page" : undefined}
                title={collapsed ? module.label : undefined}
                onClick={() => navigate(module.id)}
              >
                <Icon size={18} data-icon={module.id} />
                <span>
                  <strong>{module.label}</strong>
                  <small>{module.detail}</small>
                </span>
              </button>
            );
          })}
        </nav>
        <SidebarUtilities
          collapsed={collapsed}
          onThemeChange={setTheme}
          onToggleCollapsed={() => setCollapsed((value) => !value)}
          theme={theme}
          version={api?.bootstrap.version ?? "..."}
        />
      </aside>
      {mobileOpen && (
        <button
          className={styles.scrim}
          type="button"
          aria-label="Close navigation"
          onClick={() => setMobileOpen(false)}
        />
      )}
      <div className={styles.workspace}>
        <header className={styles.topbar}>
          <button
            className={styles.menuButton}
            type="button"
            aria-label="Open navigation"
            onClick={() => setMobileOpen(true)}
          >
            <Menu size={18} />
          </button>
          <div className={styles.pageTitle}>
            <strong>{activeModule.label}</strong>
            <span>·</span>
            <small>{activeModule.detail}</small>
          </div>
        </header>
        <main className={styles.content}>
          {startupError && (
            <div className={styles.startupError} role="alert">
              {startupError}
            </div>
          )}
          {!api && !startupError && (
            <div className={styles.boot}>
              <Box size={28} />
              <span>Connecting to AIBox Service</span>
            </div>
          )}
          {api && active === "overview" && (
            <OverviewPage
              api={api}
              operation={operation}
              onNavigate={navigate}
              onOperation={recordOperation}
            />
          )}
          {api && active === "tenants" && (
            <TenantPage
              api={api}
              locationVersion={locationVersion}
              onLocationChange={updatePageLocation}
              onOperation={recordOperation}
            />
          )}
          {api && active === "configs" && (
            <ConfigPage
              api={api}
              locationVersion={locationVersion}
              onDirtyChange={recordConfigDirty}
              onLocationChange={updatePageLocation}
            />
          )}
          {api && active === "sessions" && (
            <SessionPage
              api={api}
              locationVersion={locationVersion}
              onLocationChange={updatePageLocation}
            />
          )}
          {requestApi && active === "requests" && (
            <RequestsPage api={requestApi} standalone={false} />
          )}
        </main>
      </div>
      {api && operation && operationDismissed !== operation.id && (
        <OperationPanel
          api={api}
          operation={operation}
          onOperation={recordOperation}
          onDismiss={() => setOperationDismissed(operation.id)}
        />
      )}
    </div>
  );
}

function currentLocation(): string {
  return `${window.location.pathname}${window.location.search}${window.location.hash}`;
}

function confirmDiscardedConfig(): boolean {
  return window.confirm("Discard unsaved Config changes and continue?");
}

function mergeOperation(
  current: Operation | null,
  incoming: Operation | null,
  gap: boolean,
): Operation | null {
  if (!incoming || !current || current.id !== incoming.id || gap) return incoming;
  const logs = new Map(current.logs.map((entry) => [entry.sequence, entry]));
  for (const entry of incoming.logs) logs.set(entry.sequence, entry);
  return {
    ...incoming,
    logs: [...logs.values()]
      .filter((entry) => entry.sequence >= incoming.first_sequence)
      .sort((left, right) => left.sequence - right.sequence),
  };
}
