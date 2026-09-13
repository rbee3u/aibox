import { AlertTriangle, Box, Menu, X } from "lucide-react";
import type { CSSProperties } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { connectControlApi, type ConnectedControlApi } from "@/api/connect";
import { OperationPanel } from "@/app/OperationPanel";
import { consoleModules, moduleById, moduleFromPath } from "@/app/routing/modules";
import { modulePath } from "@/shared/lib/navigation";
import { useConsoleRouter } from "@/app/routing/useConsoleRouter";
import { SidebarUtilities } from "@/app/SidebarUtilities";
import { usePersistentTheme } from "@/app/theme/usePersistentTheme";
import { useMobileNavigation } from "@/app/useMobileNavigation";
import { useOperationFeed } from "@/app/useOperationFeed";
import { ConfigPage } from "@/features/configs/ConfigPage";
import { OverviewPage } from "@/features/overview/OverviewPage";
import type { OverviewBrowsingState } from "@/features/overview/browsingState";
import { RequestsPage } from "@/features/requests/RequestsPage";
import { SessionPage } from "@/features/sessions/SessionPage";
import { TenantPage } from "@/features/tenants/TenantPage";
import { messageOf } from "@/shared/lib/errors";
import type { ConsoleNavigate } from "@/shared/lib/navigation";
import { readPreference, storePreference } from "@/shared/lib/preferences";
import { IconButton } from "@/shared/ui/IconButton";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import styles from "@/app/App.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

const SIDEBAR_COLLAPSED_KEY = "aibox-console-sidebar-collapsed";

export function App() {
  const overviewBrowsing = useRef<OverviewBrowsingState | null>(null);
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
  const active = route.module;
  const activeModule = moduleById(active);
  const mainRef = useRef<HTMLElement>(null);
  const previousModuleRef = useRef(active);

  useEffect(() => {
    document.title = `${activeModule.label} · AIBox`;
    if (previousModuleRef.current !== active) mainRef.current?.focus();
    previousModuleRef.current = active;
  }, [active, activeModule.label]);

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
    const changesModule = module !== active;
    commitLocation(module, query);
    if (mobileLayout) closeNavigation(!changesModule);
  };

  const continuePendingNavigation = useCallback(() => {
    const pendingModule = pendingNavigation
      ? moduleFromPath(new URL(pendingNavigation, window.location.href).pathname)
      : active;
    if (!acceptPendingNavigation()) return;
    if (mobileLayout) closeNavigation(pendingModule === active);
  }, [acceptPendingNavigation, active, closeNavigation, mobileLayout, pendingNavigation]);

  return (
    <div
      data-aibox-shell="true"
      className={`${styles.app} ${collapsed ? styles.collapsed : ""}`}
      style={
        operations.visible
          ? ({ "--console-operation-height": `${operations.height}px` } as CSSProperties)
          : undefined
      }
    >
      <a
        className={styles.skipLink}
        href="#main-content"
        onClick={(event) => {
          event.preventDefault();
          document.getElementById("main-content")?.focus();
        }}
      >
        Skip to main content
      </a>
      <aside
        ref={sidebarRef}
        id="console-navigation"
        className={`${styles.sidebar} ${navigationOpen ? styles.mobileOpen : ""}`}
        aria-label="Console navigation"
        aria-hidden={mobileLayout && !navigationOpen ? "true" : undefined}
      >
        <div className={styles.brand} title={collapsed ? "AIBox · Put AI in a Box" : undefined}>
          <span className={styles.mark}>
            <Box size={iconSize.lg} />
          </span>
          <span className={styles.brandCopy}>
            <strong>AIBox</strong>
            <small>Put AI in a Box</small>
          </span>
          <IconButton
            className={styles.drawerCloseButton}
            label="Close navigation"
            onClick={() => closeNavigation()}
          >
            <X size={iconSize.md} aria-hidden="true" />
          </IconButton>
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
                <Icon size={iconSize.md} data-icon={module.id} />
                <strong>{module.label}</strong>
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
        <div
          className={styles.scrim}
          data-navigation-scrim
          aria-hidden="true"
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
            <Menu size={iconSize.md} />
          </IconButton>
          <h1 id="console-page-title" className={styles.pageTitle}>
            {activeModule.label}
          </h1>
        </header>
        <main
          ref={mainRef}
          id="main-content"
          className={styles.content}
          aria-labelledby="console-page-title"
          tabIndex={-1}
        >
          {startupError && (
            <AlertBanner
              className={styles.startupError}
              tone="danger"
              icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
            >
              {startupError}
            </AlertBanner>
          )}
          {!api && !startupError && (
            <div className={styles.boot}>
              <Box size={iconSize.xl} />
              <span>Connecting to AIBox Service</span>
            </div>
          )}
          {api && active === "overview" && (
            <OverviewPage
              browsingMemory={overviewBrowsing}
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
              onCancelLeave={cancelPendingNavigation}
              onContinueLeave={continuePendingNavigation}
              onLocationChange={locationChanges.configs}
              pendingLeave={pendingNavigation !== null}
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
          onHeightChange={operations.setHeight}
        />
      )}
    </div>
  );
}
