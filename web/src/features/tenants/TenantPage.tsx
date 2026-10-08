import { ComponentDialogs } from "@/features/tenants/components/ComponentDialogs";
import styles from "@/features/tenants/TenantPage.module.css";

import { TenantCatalogPane } from "@/features/tenants/catalog/TenantCatalogPane";
import { TenantDetailPane } from "@/features/tenants/components/TenantDetailPane";
import { TenantDialogs } from "@/features/tenants/mutation/TenantDialogs";
import { useTenantController } from "@/features/tenants/useTenantController";

import { MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";
import layout from "@/shared/ui/layout/catalog.module.css";
import type { TenantPageProps } from "@/features/tenants/viewTypes";

export function TenantPage(props: TenantPageProps) {
  const viewModel = useTenantController(props);
  const { catalog, components, detail, dialogs, feedback, mutations, selection } = viewModel;
  return (
    <div className={`${layout.page} ${layout.catalogPage}`}>
      <PageError
        error={feedback.error ?? catalog.tenantCatalogError}
        onRetry={() => void catalog.retryTenantPage()}
      />
      <MutationUnavailable operation={props.operation} />
      <div className={`${styles.splitLayout} ${detail.detailOpen ? layout.showsDetail : ""}`}>
        <TenantCatalogPane
          catalog={catalog}
          detail={detail}
          dialogs={dialogs}
          feedback={feedback}
          mutations={mutations}
          onLocationChange={props.onLocationChange}
          selection={selection}
        />
        <TenantDetailPane
          components={components}
          detail={detail}
          dialogs={dialogs}
          mutations={mutations}
          onLocationChange={props.onLocationChange}
          selection={selection}
        />
      </div>
      <NotificationCenter
        notifications={feedback.notifications}
        paused={dialogs.createOpen || dialogs.deleteTarget !== null}
        onAction={() => undefined}
        onDismiss={feedback.dismissNotification}
      />
      <TenantDialogs dialogs={dialogs} mutations={mutations} />
      <ComponentDialogs components={components} dialogs={dialogs} mutations={mutations} />
    </div>
  );
}
