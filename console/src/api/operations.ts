import type { ControlApi } from "@/api/transport";

export interface OperationLog {
  sequence: number;
  message: string;
}

export interface Operation {
  id: string;
  kind: string;
  state: "running" | "succeeded" | "failed" | "cancelled";
  started_at: string;
  ended_at: string | null;
  result: string | null;
  first_sequence: number;
  next_sequence: number;
  logs: OperationLog[];
}

export interface OperationApi {
  current(): Promise<Operation | null>;
  cancel(id: string): Promise<void>;
  subscribe(handlers: {
    onConnection: (state: "connected" | "reconnecting") => void;
    onOperation: (operation: Operation | null, gap: boolean) => void;
  }): () => void;
}

export function operationsApi(client: ControlApi): OperationApi {
  return {
    current: () =>
      client
        .get<{ operation: Operation | null }>("/_aibox/api/operations/current")
        .then((value) => value.operation),
    cancel: async (id) => {
      await client.post(`/_aibox/api/operations/${encodeURIComponent(id)}/cancel`);
    },
    subscribe: (handlers) => {
      const source = new EventSource("/_aibox/api/operations/events");
      source.addEventListener("open", () => handlers.onConnection("connected"));
      source.addEventListener("error", () => handlers.onConnection("reconnecting"));
      source.addEventListener("operation", (event) => {
        const value = JSON.parse((event as MessageEvent<string>).data) as {
          operation: Operation | null;
          gap: boolean;
        };
        handlers.onOperation(value.operation, value.gap);
      });
      return () => source.close();
    },
  };
}
