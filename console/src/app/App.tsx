import { AlertTriangle, Box, Menu } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { connectControlApi, type ConnectedControlApi } from "@/api/connect";
import { OperationPanel } from "@/app/OperationPanel";
import { consoleModules, moduleById, modulePath } from "@/app/routing/modules";
import { useConsoleRouter } from "@/app/routing/useConsoleRouter";
import { SidebarUtilities } from "@/app/SidebarUtilities";
import { usePersistentTheme } from "@/app/theme/usePersistentTheme";
import { useMobileNavigation } from "@/app/useMobileNavigation";
import { useOperationFeed } from "@/app/useOperationFeed";
import { ConfigPage } from "@/features/configs/ConfigPage";
import { OverviewPage } from "@/features/overview/OverviewPage";
import { RequestsPage } from "@/features/requests/RequestsPage";
import { SessionPage } from "@/features/sessions/SessionPage";
import { TenantPage } from "@/features/tenants/TenantPage";
import { messageOf } from "@/shared/lib/errors";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import { readPreference, storePreference } from "@/shared/lib/preferences";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { IconButton } from "@/shared/ui/IconButton";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import styles from "@/app/App.module.css";

const SIDEBAR_COLLAPSED_KEY = "aibox-console-sidebar-collapsed";

export function App() {
  const [api, setApi] = useState<ConnectedControlApi | null>(null);
  const [startupError, setStartupError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(
    () => readPreference(SIDEBAR_COLLAPSED_KEY) === "true",
  );
  const [theme, setTheme] = usePersistentTheme();
  const {
    open: navigationOpen,
    setOpen: setNavigationOpen,
    close: closeNavigation,
    mobileLayout,
    sidebarRef,
    menuButtonRef,
  } = useMobileNavigation();
  const operations = useOperationFeed(api);
  const {
    route,
    commitLocation,
    locationChanges,
    recordDirty,
    isDirty,
    requestNavigation,
    pendingNavigation,
    cancelPendingNavigation,
    acceptPendingNavigation,
  } = useConsoleRouter();

  useEffect(() => {
    void connectControlApi()
      .then((client) => {
        setApi(client);
        return client.operations.current();
      })
      .then(operations.adopt)
      .catch((cause: unknown) => setStartupError(messageOf(cause)));
    // The Control API connects once for the lifetime of the shell.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    storePreference(SIDEBAR_COLLAPSED_KEY, String(collapsed));
  }, [collapsed]);

  const navigate: ConsoleNavigate = (module, query) => {
    if (isDirty()) {
      requestNavigation(modulePath(module, query));
      return;
    }
    commitLocation(module, query);
    if (mobileLayout) closeNavigation();
  };

  const continuePendingNavigation = useCallback(() => {
    if (!acceptPendingNavigation()) return;
    if (mobileLayout) closeNavigation();
  }, [acceptPendingNavigation, closeNavigation, mobileLayout]);

  const active = route.module;
  const activeModule = moduleById(active);

  return (
    <div
      data-aibox-shell="true"
      className={`${styles.app} ${collapsed ? styles.collapsed : ""} ${operations.visible && operations.expanded ? styles.operationExpanded : ""}`}
    >
      <aside
        ref={sidebarRef}
        id="console-navigation"
        className={`${styles.sidebar} ${navigationOpen ? styles.mobileOpen : ""}`}
        aria-label="Console navigation"
        aria-hidden={mobileLayout && !navigationOpen ? "true" : undefined}
      >
        <div className={styles.brand} title={collapsed ? "AIBox · Put AI in a Box" : undefined}>
          <span className={styles.mark}>
            <Box size={23} strokeWidth={2.2} />
          </span>
          <span className={styles.brandCopy}>
            <strong>AIBox</strong>
            <small>Put AI in a Box</small>
          </span>
        </div>
        <nav className={styles.moduleNav} aria-label="Modules">
          {consoleModules.map((module) => {
            const Icon = module.icon;
            return (
              <a
                key={module.id}
                href={modulePath(module.id)}
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
      {navigationOpen && (
        <button
          className={styles.scrim}
          type="button"
          aria-label="Close navigation"
          onClick={() => closeNavigation()}
        />
      )}
      <div className={styles.workspace}>
        <header className={styles.topbar}>
          <IconButton
            buttonRef={menuButtonRef}
            className={styles.menuButton}
            label="Open navigation"
            aria-controls="console-navigation"
            aria-expanded={navigationOpen}
            onClick={() => setNavigationOpen(true)}
          >
            <Menu size={18} />
          </IconButton>
          <div className={styles.pageTitle}>
            <h1>{activeModule.label}</h1>
            <small>{activeModule.detail}</small>
          </div>
        </header>
        <main className={styles.content}>
          {startupError && (
            <AlertBanner
              className={styles.startupError}
              tone="danger"
              icon={<AlertTriangle size={16} aria-hidden="true" />}
            >
              {startupError}
            </AlertBanner>
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
              operation={operations.operation}
              onNavigate={navigate}
              onOperation={operations.record}
            />
          )}
          {api && active === "tenants" && (
            <TenantPage
              api={api.tenants}
              operation={operations.operation}
              search={route.search}
              onLocationChange={locationChanges.tenants}
              onOperation={operations.record}
            />
          )}
          {api && active === "configs" && (
            <ConfigPage
              api={api.configs}
              operation={operations.operation}
              search={route.search}
              onDirtyChange={recordDirty}
              onLocationChange={locationChanges.configs}
            />
          )}
          {api && active === "sessions" && (
            <SessionPage
              api={api.sessions}
              operation={operations.operation}
              search={route.search}
              onLocationChange={locationChanges.sessions}
            />
          )}
          {api && active === "requests" && (
            <RequestsPage
              api={api.requests}
              search={route.search}
              onLocationChange={locationChanges.requests}
            />
          )}
        </main>
      </div>
      {api && operations.visible && (
        <OperationPanel
          api={api.operations}
          operation={operations.visible}
          connection={operations.connection}
          onOperation={operations.record}
          onDismiss={operations.dismiss}
          onExpandedChange={operations.setExpanded}
        />
      )}
      {pendingNavigation && (
        <ConfirmDialog
          title="Discard unsaved Config changes?"
          message="Unsaved Config changes will be lost if you continue."
          confirmLabel="Discard and continue"
          variant="primary"
          onCancel={cancelPendingNavigation}
          onConfirm={continuePendingNavigation}
        />
      )}
    </div>
  );
}
