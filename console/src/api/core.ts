export type CodingAgentKind = "codex" | "claude";

export interface Bootstrap {
  version: string;
  csrf_token: string;
  listen: string;
}

interface TenantRowBase {
  display_name: string;
  home: string;
  exists: boolean;
}

export type TenantRow =
  | (TenantRowBase & { kind: "host"; name: null })
  | (TenantRowBase & { kind: "managed"; name: string });
