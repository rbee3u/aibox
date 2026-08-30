import type { Operation } from "@/api/operations";
import type { TenantApi } from "@/api/tenants";
import { TenantCatalogPane } from "@/features/tenants/catalog/TenantCatalogPane";
import { TenantDetailPane } from "@/features/tenants/detail/TenantDetailPane";
import { TenantDialogs } from "@/features/tenants/mutation/TenantDialogs";
import { useTenantController } from "@/features/tenants/useTenantController";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/tenants/TenantPage.module.css";

interface PageProps {
  api: TenantApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}

export function TenantPage(props: PageProps) {
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
      <TenantDialogs components={components} dialogs={dialogs} mutations={mutations} />
    </div>
  );
}
