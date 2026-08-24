import { Box, Menu } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { ConfigPage } from "./ConfigPage";
import { OperationPanel } from "./OperationPanel";
import { SessionPage } from "./SessionPage";
import { TenantPage } from "./TenantPage";
import { OverviewPage, type ConsoleNavigate } from "./OverviewPage";
import { RequestsPage } from "./RequestsPage";
import { connectControlApi } from "./controlApi";
import type { ConnectedControlApi, Operation } from "./controlApi";
import { IconButton } from "./components/IconButton";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { readPreference, storePreference } from "./preferences";
import { SidebarUtilities } from "./SidebarUtilities";
import { usePersistentTheme } from "./usePersistentTheme";
import { moduleIcons, type ModuleId } from "./consoleIcons";
import styles from "./App.module.css";
const modules = [
  { id: "overview", label: "Overview", detail: "Current status", icon: moduleIcons.overview },
  { id: "tenants", label: "Tenants", detail: "Identity & Components", icon: moduleIcons.tenants },
  { id: "configs", label: "Configs", detail: "Native configuration", icon: moduleIcons.configs },
  {
    id: "sessions",
    label: "Sessions",
    detail: "Coding Agent transcripts",
    icon: moduleIcons.sessions,
  },
  {
    id: "requests",
    label: "Requests",
    detail: "Request diagnostics",
    icon: moduleIcons.requests,
  },
] as const;
const SIDEBAR_COLLAPSED_KEY = "aibox-console-sidebar-collapsed";
interface RouteSnapshot {
  module: ModuleId;
  search: string;
}
function moduleFromPath(pathname: string): ModuleId {
  const value = pathname.split("/").filter(Boolean).at(-1);
  return modules.some((module) => module.id === value) ? (value as ModuleId) : "overview";
}
function currentRoute(): RouteSnapshot {
  return { module: moduleFromPath(window.location.pathname), search: window.location.search };
}
export function App() {
  const [api, setApi] = useState<ConnectedControlApi | null>(null);
  const [route, setRoute] = useState<RouteSnapshot>(currentRoute);
  const [collapsed, setCollapsed] = useState(
    () => readPreference(SIDEBAR_COLLAPSED_KEY) === "true",
  );
  const [mobileOpen, setMobileOpen] = useState(false);
  const [mobileLayout, setMobileLayout] = useState(
    () => window.matchMedia?.("(max-width: 900px)").matches ?? false,
  );
  const [theme, setTheme] = usePersistentTheme();
  const [operation, setOperation] = useState<Operation | null>(null);
  const [operationConnection, setOperationConnection] = useState<
    "connecting" | "connected" | "reconnecting"
  >("connecting");
  const [operationDismissed, setOperationDismissed] = useState<string | null>(null);
  const [operationExpanded, setOperationExpanded] = useState(false);
  const [startupError, setStartupError] = useState<string | null>(null);
  const [pendingNavigation, setPendingNavigation] = useState<string | null>(null);
  const configDirty = useRef(false);
  const acceptedLocation = useRef(currentLocation());
  const sidebarRef = useRef<HTMLElement>(null);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    void connectControlApi()
      .then((client) => {
        setApi(client);
        return client.operations.current();
      })
      .then(setOperation)
      .catch((cause: unknown) =>
        setStartupError(cause instanceof Error ? cause.message : String(cause)),
      );
  }, []);
  useEffect(() => {
    if (!api) return;
    return api.operations.subscribe({
      onConnection: setOperationConnection,
      onOperation: (value, gap) => {
        setOperation((current) => mergeOperation(current, value, gap));
        if (value?.state === "running") setOperationDismissed(null);
      },
    });
  }, [api]);
  useEffect(() => {
    const onPopState = () => {
      const next = currentLocation();
      if (configDirty.current && !confirmDiscardedConfig()) {
        window.history.pushState(null, "", acceptedLocation.current);
        return;
      }
      acceptedLocation.current = next;
      setRoute(currentRoute());
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
  useEffect(() => {
    if (!window.matchMedia) return;
    const query = window.matchMedia("(max-width: 900px)");
    const update = () => {
      setMobileLayout(query.matches);
      if (!query.matches) setMobileOpen(false);
    };
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  const closeMobileNavigation = useCallback((restoreFocus = true) => {
    setMobileOpen(false);
    if (restoreFocus) window.requestAnimationFrame(() => menuButtonRef.current?.focus());
  }, []);
  useEffect(() => {
    const sidebar = sidebarRef.current;
    if (!sidebar) return;
    sidebar.inert = mobileLayout && !mobileOpen;
    if (!mobileLayout || !mobileOpen) return;
    const focusable = () =>
      [...sidebar.querySelectorAll<HTMLElement>("a[href], button:not([disabled]), select")].filter(
        (element) => !element.hidden && element.tabIndex >= 0,
      );
    window.requestAnimationFrame(() =>
      (sidebar.querySelector<HTMLElement>('[aria-current="page"]') ?? focusable()[0])?.focus(),
    );
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === "Escape") {
        event.preventDefault();
        closeMobileNavigation();
        return;
      }
      if (event.key !== "Tab") return;
      const elements = focusable();
      const first = elements[0];
      const last = elements.at(-1);
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [closeMobileNavigation, mobileLayout, mobileOpen]);
  const active = route.module;
  const activeModule = modules.find((module) => module.id === active)!;
  const commitLocation = useCallback(
    (module: ModuleId, query?: URLSearchParams, replace = false) => {
      const suffix = query?.toString();
      const next = `/_aibox/ui/${module}${suffix ? `?${suffix}` : ""}`;
      window.history[replace ? "replaceState" : "pushState"](null, "", next);
      acceptedLocation.current = next;
      setRoute({ module, search: suffix ? `?${suffix}` : "" });
    },
    [],
  );
  const navigate: ConsoleNavigate = (module, query) => {
    const suffix = query?.toString();
    const next = `/_aibox/ui/${module}${suffix ? `?${suffix}` : ""}`;
    if (configDirty.current) {
      setPendingNavigation(next);
      return;
    }
    commitLocation(module, query);
    if (mobileLayout) closeMobileNavigation();
  };
  const continuePendingNavigation = useCallback(() => {
    if (!pendingNavigation) return;
    window.history.pushState(null, "", pendingNavigation);
    acceptedLocation.current = pendingNavigation;
    setRoute(currentRoute());
    setPendingNavigation(null);
    if (mobileLayout) closeMobileNavigation();
  }, [closeMobileNavigation, mobileLayout, pendingNavigation]);
  const updatePageLocation = useCallback(
    (module: ModuleId, query: URLSearchParams, replace = false) => {
      commitLocation(module, query, replace);
    },
    [commitLocation],
  );
  const updateRequestsLocation = useCallback(
    (query: URLSearchParams, replace = false) => updatePageLocation("requests", query, replace),
    [updatePageLocation],
  );
  const recordConfigDirty = useCallback((dirty: boolean) => {
    configDirty.current = dirty;
  }, []);
  function recordOperation(value: Operation) {
    setOperation(value);
    setOperationDismissed(null);
  }
  const visibleOperation =
    api && operation && operationDismissed !== operation.id ? operation : null;
  return (
    <div
      data-aibox-shell="true"
      className={`${styles.app} ${collapsed ? styles.collapsed : ""} ${visibleOperation && operationExpanded ? styles.operationExpanded : ""}`}
    >
      <aside
        ref={sidebarRef}
        id="console-navigation"
        className={`${styles.sidebar} ${mobileOpen ? styles.mobileOpen : ""}`}
        aria-label="Console navigation"
        aria-hidden={mobileLayout && !mobileOpen ? "true" : undefined}
      >
        <div className={styles.brand} title={collapsed ? "AIBox · Put AI in a Box" : undefined}>
          <span className={styles.mark}>
            <Box size={23} strokeWidth={2.2} />
          </span>
          <span className={styles.brandCopy}>
            <strong>AIBox</strong>
            <small>· Put AI in a Box</small>
          </span>
        </div>
        <nav className={styles.moduleNav} aria-label="Modules">
          {modules.map((module) => {
            const Icon = module.icon;
            return (
              <a
                key={module.id}
                href={`/_aibox/ui/${module.id}`}
                aria-current={active === module.id ? "page" : undefined}
                title={collapsed ? module.label : undefined}
                onClick={(event) => {
                  if (
                    event.defaultPrevented ||
                    event.button !== 0 ||
                    event.metaKey ||
                    event.ctrlKey ||
                    event.shiftKey ||
                    event.altKey
                  )
                    return;
                  event.preventDefault();
                  navigate(module.id);
                }}
              >
                <Icon size={18} data-icon={module.id} />
                <span>
                  <strong>{module.label}</strong>
                  <small>{module.detail}</small>
                </span>
              </a>
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
          onClick={() => closeMobileNavigation()}
        />
      )}
      <div className={styles.workspace}>
        <header className={styles.topbar}>
          <IconButton
            buttonRef={menuButtonRef}
            className={styles.menuButton}
            label="Open navigation"
            aria-controls="console-navigation"
            aria-expanded={mobileOpen}
            onClick={() => setMobileOpen(true)}
          >
            <Menu size={18} />
          </IconButton>
          <div className={styles.pageTitle}>
            <h1>{activeModule.label}</h1>
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
              api={api.overview}
              operation={operation}
              onNavigate={navigate}
              onOperation={recordOperation}
            />
          )}
          {api && active === "tenants" && (
            <TenantPage
              api={api.tenants}
              operation={operation}
              search={route.search}
              onLocationChange={updatePageLocation}
              onOperation={recordOperation}
            />
          )}
          {api && active === "configs" && (
            <ConfigPage
              api={api.configs}
              operation={operation}
              search={route.search}
              onDirtyChange={recordConfigDirty}
              onLocationChange={updatePageLocation}
            />
          )}
          {api && active === "sessions" && (
            <SessionPage
              api={api.sessions}
              operation={operation}
              search={route.search}
              onLocationChange={updatePageLocation}
            />
          )}
          {api && active === "requests" && (
            <RequestsPage
              api={api.requests}
              search={route.search}
              onLocationChange={updateRequestsLocation}
            />
          )}
        </main>
      </div>
      {api && visibleOperation && (
        <OperationPanel
          api={api.operations}
          operation={visibleOperation}
          connection={operationConnection}
          onOperation={recordOperation}
          onDismiss={() => setOperationDismissed(visibleOperation.id)}
          onExpandedChange={setOperationExpanded}
        />
      )}
      {pendingNavigation && (
        <ConfirmDialog
          title="Discard unsaved Config changes?"
          message="Your unsaved Config changes will be lost if you continue."
          confirmLabel="Discard and continue"
          variant="primary"
          onCancel={() => setPendingNavigation(null)}
          onConfirm={continuePendingNavigation}
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
