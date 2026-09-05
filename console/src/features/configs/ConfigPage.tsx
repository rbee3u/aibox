import type { ConfigApi } from "@/api/configs";
import type { Operation } from "@/api/operations";
import { ConfigCatalogPane } from "@/features/configs/catalog/ConfigCatalogPane";
import { ConfigDetailPane } from "@/features/configs/detail/ConfigDetailPane";
import { ConfigDialogs } from "@/features/configs/mutation/ConfigDialogs";
import { useConfigController } from "@/features/configs/useConfigController";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import layout from "@/shared/ui/layout/catalog.module.css";

interface PageProps {
  api: ConfigApi;
  operation?: Operation | null;
  search: string;
  onDirtyChange?: (dirty: boolean) => void;
  onCancelLeave?: () => void;
  onContinueLeave?: () => void | Promise<void>;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
  pendingLeave?: boolean;
}

export function ConfigPage(props: PageProps) {
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
          editor={editor}
          feedback={feedback}
          mutations={mutations}
        />
      </div>
      <ConfigDialogs catalog={catalog} dialogs={dialogs} editor={editor} mutations={mutations} />
    </div>
  );
}
