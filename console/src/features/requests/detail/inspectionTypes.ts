import type { BodyKind } from "@/api/requests";

export interface RequestInspectionIdentity {
  id: string;
  generation: number;
}

export interface InspectionFailure {
  kind: "detail" | "body" | "download";
  message: string;
  bodyKind?: BodyKind;
  retryable?: boolean;
}

export type ReportInspectionFailure = (failure: InspectionFailure) => void;
export type ClearInspectionFailure = (kind?: InspectionFailure["kind"]) => void;
