import type { Operation } from "@/api/operations";
import type { SessionApi } from "@/api/sessions";
import { SessionCatalogPane } from "@/features/sessions/catalog/SessionCatalogPane";
import { SessionDetailPane } from "@/features/sessions/detail/SessionDetailPane";
import { SessionDialogs } from "@/features/sessions/mutation/SessionDialogs";
import { useSessionController } from "@/features/sessions/useSessionController";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/sessions/SessionPage.module.css";

interface PageProps {
  api: SessionApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
}

export function SessionPage(props: PageProps) {
  const viewModel = useSessionController(props);
  const { catalog, detail, dialogs, feedback, mutations, selection } = viewModel;
  return (
    <div className={`${layout.page} ${layout.catalogPage} ${styles.sessionPage}`}>
      <PageError
        error={catalog.tenantError ?? feedback.error}
        onRetry={
          catalog.tenantError
            ? catalog.retryTenants
            : feedback.error
              ? catalog.retryPageError
              : undefined
        }
      />
      <MutationUnavailable operation={props.operation} />
      <div className={`${styles.splitLayout} ${detail.currentSession ? layout.showsDetail : ""}`}>
        <SessionCatalogPane
          catalog={catalog}
          detail={detail}
          dialogs={dialogs}
          mutations={mutations}
          selection={selection}
        />
        <SessionDetailPane api={props.api} detail={detail} mutations={mutations} />
      </div>
      <SessionDialogs
        dialogs={dialogs}
        feedback={feedback}
        mutations={mutations}
        operation={props.operation}
      />
    </div>
  );
}
