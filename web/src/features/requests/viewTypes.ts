import type { BodyKind } from "@/api/requests";

export type BodyLoadStatus = "idle" | "loaded" | "error";
export type DetailTab = "summary" | BodyKind;

export interface DecodedBodyState {
  bytes: Uint8Array | null;
  error: string | null;
}
