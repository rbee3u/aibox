import type { RequestSummary } from "@/api/requests";
import { requestUrl } from "@/features/requests/requestFormat";
import { requestStatusPresentation } from "@/features/requests/statusPresentation";
import { formatTimestamp } from "@/shared/lib/format";
import type { ConfirmDialogFact } from "@/shared/ui/ConfirmDialog";

/** Catalog-row facts for a single-request delete confirmation. */
export function requestDeletionFacts(
  request: RequestSummary | undefined,
  id: string,
): ConfirmDialogFact[] {
  if (!request) {
    return [{ label: "Id", value: id }];
  }

  const target = requestUrl(request);
  const status = requestStatusPresentation({
    status: request.status,
    state: request.state,
    assessment: request.assessment,
  });
  const timestampKind = request.ended_at ? "Ended" : "Started";
  const timestampValue = request.ended_at ?? request.started_at;

  return [
    { label: "Request", value: `${request.method} ${target.label}` },
    { label: "Status", value: status.label },
    { label: timestampKind, value: formatTimestamp(timestampValue) },
    { label: "Id", value: request.id },
  ];
}
