import styles from "@/features/requests/detail/TokenUsageGroup.module.css";
import summarySharedStyles from "@/features/requests/detail/summaryShared.module.css";

import type { RequestDetail as RequestDetailData } from "@/api/requests";

import { tokenCount } from "@/features/requests/summary";

export function TokenUsageGroup({ detail }: { detail: RequestDetailData }) {
  const protocol = detail.summary.protocol;
  const usage = protocol?.token_usage ?? null;
  const claude = protocol?.family === "claude_messages";
  const hasCacheWriteBreakdown =
    claude && (usage?.cache_write_5m_tokens != null || usage?.cache_write_1h_tokens != null);
  const cacheWrites = hasCacheWriteBreakdown
    ? (usage?.cache_write_5m_tokens ?? 0) + (usage?.cache_write_1h_tokens ?? 0)
    : (usage?.cache_write_tokens ?? null);
  const inputMetrics: Array<{
    label: string;
    value: number | null;
    details?: Array<{ label: string; value: number | null }>;
  }> = [
    {
      label: claude ? "Base input" : "Input",
      value: usage?.base_input_tokens ?? null,
    },
    {
      label: claude ? "Cache hits & refreshes" : "Cached input",
      value: usage?.cached_input_tokens ?? null,
    },
    {
      label: "Cache writes",
      value: cacheWrites,
      details: hasCacheWriteBreakdown
        ? [
            { label: "5m", value: usage?.cache_write_5m_tokens ?? null },
            { label: "1h", value: usage?.cache_write_1h_tokens ?? null },
          ]
        : undefined,
    },
  ];
  const totalInput = usage?.total_input_tokens ?? null;
  const output = usage?.output_tokens ?? null;
  const reasoning = output !== null ? (usage?.reasoning_output_tokens ?? null) : null;
  const hasUsageData = [
    usage?.total_input_tokens,
    usage?.base_input_tokens,
    usage?.cached_input_tokens,
    usage?.cache_write_tokens,
    usage?.cache_write_5m_tokens,
    usage?.cache_write_1h_tokens,
    usage?.output_tokens,
  ].some((value) => value != null);
  return (
    <section className={summarySharedStyles.tokenSection} aria-labelledby="request-token-title">
      <h2 id="request-token-title">Token usage</h2>
      {hasUsageData ? (
        <div className={styles.tokenUsageGrid}>
          <div className={styles.tokenCard} role="group" aria-label="Input tokens container">
            <dl className={styles.tokenCardHeader} role="group" aria-label="Total input tokens">
              <div className={styles.tokenMetricPrimary}>
                <dt>Total input</dt>
                <dd>{displayTokenCount(totalInput)}</dd>
              </div>
            </dl>
            <div className={styles.tokenSubMetrics} role="group" aria-label="Input tokens">
              {inputMetrics.map((metric) => (
                <div
                  className={`${styles.tokenSubCell} ${
                    metric.details ? styles.tokenSubCellDetailed : ""
                  }`}
                  role="group"
                  aria-label={`${metric.label} billing category`}
                  key={metric.label}
                >
                  <dl className={styles.tokenSubDl}>
                    <div>
                      <dt>{metric.label}</dt>
                      <dd>{displayTokenCount(metric.value)}</dd>
                    </div>
                  </dl>
                  {metric.details && (
                    <dl
                      className={styles.tokenCacheBreakdown}
                      role="group"
                      aria-label="Cache write TTL breakdown"
                    >
                      {metric.details.map((detailMetric) => (
                        <div className={styles.tokenCacheDetail} key={detailMetric.label}>
                          <dt>{detailMetric.label}</dt>
                          <dd>{displayTokenCount(detailMetric.value)}</dd>
                        </div>
                      ))}
                    </dl>
                  )}
                </div>
              ))}
            </div>
          </div>
          <div className={styles.tokenCard} role="group" aria-label="Output tokens">
            <dl className={styles.tokenCardHeader}>
              <div className={styles.tokenMetricPrimary}>
                <dt>Output</dt>
                <dd>{displayTokenCount(output)}</dd>
              </div>
            </dl>
            {reasoning !== null && (
              <div className={styles.tokenSubMetrics}>
                <div
                  className={styles.tokenSubCell}
                  role="group"
                  aria-label={`Output includes ${tokenCount(reasoning)} reasoning tokens`}
                >
                  <dl className={styles.tokenSubDl}>
                    <div>
                      <dt>Reasoning</dt>
                      <dd>{tokenCount(reasoning)}</dd>
                    </div>
                  </dl>
                </div>
              </div>
            )}
          </div>
        </div>
      ) : (
        <p className={styles.usageMessage}>{usageStateMessage(detail)}</p>
      )}
    </section>
  );
}

function usageStateMessage(detail: RequestDetailData): string {
  const protocol = detail.summary.protocol;
  if (!protocol || protocol.family === "unknown") {
    return "Token usage is unavailable for this protocol.";
  }
  if (protocol.token_usage) return "The upstream API reported no token counters.";
  if (detail.state === "active" && !protocol.response_terminal) {
    return "Waiting for the upstream API to report token usage.";
  }
  if (
    detail.state !== "active" &&
    (detail.summary.outcome !== "completed" || !protocol.response_terminal)
  ) {
    return "Token usage was not reported before this request ended.";
  }
  return "The completed response did not report token usage.";
}

function displayTokenCount(value: number | null): string {
  return value === null ? "—" : tokenCount(value);
}
