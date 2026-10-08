import { ConfigCatalogPane } from "@/features/configs/catalog/ConfigCatalogPane";
import { ConfigDetailPane } from "@/features/configs/editor/ConfigDetailPane";
import { ConfigDialogs } from "@/features/configs/mutation/ConfigDialogs";
import { useConfigController } from "@/features/configs/useConfigController";

import { MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";
import layout from "@/shared/ui/layout/catalog.module.css";
import type { ConfigPageProps } from "@/features/configs/viewTypes";

export function ConfigPage(props: ConfigPageProps) {
  const viewModel = useConfigController(props);
  const { catalog, detail, dialogs, editor, feedback, mutations, selection } = viewModel;
  return (
    <div className={`${layout.page} ${layout.catalogPage}`}>
      <PageError
        error={catalog.tenantError ?? catalog.catalogError ?? feedback.error}
        onRetry={
          catalog.tenantError
            ? catalog.retryTenants
            : catalog.catalogError || feedback.error
              ? () => {
                  feedback.setError(null);
                  editor.retryReveals();
                  void catalog.loadCatalog("refresh");
                }
              : undefined
        }
      />
      <MutationUnavailable operation={props.operation} />
      <div className={`${layout.splitLayout} ${detail.detailOpen ? layout.showsDetail : ""}`}>
        <ConfigCatalogPane
          catalog={catalog}
          detail={detail}
          dialogs={dialogs}
          editor={editor}
          feedback={feedback}
          mutations={mutations}
          selection={selection}
        />
        <ConfigDetailPane
          api={props.api}
          catalog={catalog}
          detail={detail}
          dialogs={dialogs}
          editor={editor}
          feedback={feedback}
          mutations={mutations}
        />
      </div>
      <NotificationCenter
        notifications={feedback.notifications}
        paused={dialogs.createOpen || dialogs.deleteTarget !== null || dialogs.applyTarget !== null}
        onAction={() => undefined}
        onDismiss={feedback.dismissNotification}
      />
      <ConfigDialogs catalog={catalog} dialogs={dialogs} editor={editor} mutations={mutations} />
    </div>
  );
}
