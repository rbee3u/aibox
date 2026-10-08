import type { ControlApi } from "@/api/transport";
import type {
  OperationEnvelope,
  OperationLog,
  OperationSnapshot,
  OperationState,
} from "@/api/generated/wire";

export type { OperationLog, OperationState };
export type Operation = OperationSnapshot;

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
    current: async () => {
      const response = await client.get<OperationEnvelope>("/_aibox/api/operations/current");
      return response.operation;
    },
    cancel: async (id) => {
      await client.post(`/_aibox/api/operations/${encodeURIComponent(id)}/cancel`);
    },
    subscribe: (handlers) => {
      const source = new EventSource("/_aibox/api/operations/events");
      source.addEventListener("open", () => handlers.onConnection("connected"));
      source.addEventListener("error", () => handlers.onConnection("reconnecting"));
      source.addEventListener("operation", (event) => {
        const envelope = JSON.parse((event as MessageEvent<string>).data) as OperationEnvelope;
        handlers.onOperation(envelope.operation, envelope.gap);
      });
      return () => source.close();
    },
  };
}
