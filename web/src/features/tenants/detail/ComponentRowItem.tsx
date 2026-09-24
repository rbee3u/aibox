import { ArrowUp, Check, Download, LoaderCircle, RefreshCw, Trash2 } from "lucide-react";
import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import type { ComponentKind, ComponentRow } from "@/api/tenants";
import { ComponentGlyph } from "@/features/tenants/detail/ComponentGlyph";
import { compareStableVersions, type ComponentRowModel } from "@/features/tenants/componentCatalog";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import { StatusBadge, type StatusTone, type StatusVariant } from "@/shared/ui/StatusBadge";
import styles from "@/features/tenants/TenantPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

export interface ComponentRowItemProps {
  row: ComponentRow;
  model: ComponentRowModel;
  /** Label of the Operation running for this row, if any. */
  progressLabel: string | null;
  busy: boolean;
  mutationBusy: boolean;
  openMenu?: ComponentKind | null;
  menuPosition?: { top: number; left: number } | null;
  menuRef?: RefObject<HTMLDivElement | null>;
  onRetryInspection: () => void;
  onInstall: (version?: string | null) => void;
  onRemove: () => void;
  onOpenSpecificVersion?: () => void;
  onCloseMenu?: () => void;
  onOpenMenu?: (anchor: HTMLButtonElement) => void;
  onToggleMenu?: (anchor: HTMLButtonElement) => void;
  registerMenuButton?: (element: HTMLButtonElement | null) => void;
  registerMenuItem?: (element: HTMLButtonElement | null) => void;
  highlighted?: boolean;
}

export interface ComponentAgentCardProps {
  agent: ComponentRowItemProps;
  statusline: {
    row: ComponentRow;
    model: ComponentRowModel;
    progressLabel: string | null;
    busy: boolean;
    mutationBusy: boolean;
    highlighted?: boolean;
    onRetryInspection: () => void;
    onInstall: () => void;
    onRemove: () => void;
  } | null;
}

function ComponentCardInner({
  row,
  model,
  progressLabel,
  busy,
  mutationBusy,
  onRetryInspection,
  onInstall,
  onRemove,
}: ComponentRowItemProps) {
  const { label, presentation, latest, diagnostic, primaryAction } = model;
  const defaultTargetVersion = latest.latestVersion ?? "";
  const [customVersion, setCustomVersion] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const effectiveVersion = customVersion !== null ? customVersion : defaultTargetVersion;

  const handleVersionChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value;
    const normalized = raw.replace(/^v/i, "").trimStart();
    setCustomVersion(normalized);
  };

  const trimmedVersion = effectiveVersion.trim();
  const formatValid =
    trimmedVersion.length === 0
      ? primaryAction === "Install"
      : /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(trimmedVersion);
  let validationError: string | null = null;
  if (trimmedVersion.length > 0 && !formatValid) {
    validationError = "Enter a stable version in X.Y.Z form.";
  } else if (formatValid && trimmedVersion.length > 0 && primaryAction === "Update") {
    const currentVersion = row.version;
    const currentComparison = currentVersion
      ? compareStableVersions(trimmedVersion, currentVersion)
      : null;
    if (currentComparison === 0) {
      validationError = `Version v${currentVersion} is already installed.`;
    } else if (currentComparison === -1) {
      validationError = `Enter a version newer than v${currentVersion}. Remove the Component before installing a lower version.`;
    } else if (currentComparison === null) {
      validationError = "The installed version cannot be compared safely.";
    }
  }
  const isValid = formatValid && !validationError;

  const handleInstallSubmit = (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!isValid || mutationBusy) return;
    onInstall(trimmedVersion.length > 0 ? trimmedVersion : null);
  };

  const isOutdated = row.status === "installed" && row.supports_version && latest.updateAvailable;

  const stateLabel = isOutdated ? "Outdated" : (presentation.stateBadge ?? presentation.stateLabel);

  const stateTone: StatusTone =
    row.error || !row.status
      ? "error"
      : isOutdated
        ? "warning"
        : row.status === "installed"
          ? "good"
          : row.status === "not-installed"
            ? "neutral"
            : "warning";
  const stateVariant: StatusVariant =
    isOutdated || row.status === "installed" || row.status === "not-installed" ? "inline" : "badge";

  return (
    <>
      <div className={styles.componentCardHeader}>
        <div className={styles.componentHeaderLeft}>
          <span className={styles.componentIconTile} data-component-icon={row.kind}>
            <ComponentGlyph kind={row.kind} />
          </span>
          <div className={styles.componentIdentity}>
            <strong title={label}>{label}</strong>
            {row.supports_version &&
              row.version &&
              (row.status === "installed" || row.status === "modified") && (
                <span className={styles.componentHeaderVersion}>v{row.version}</span>
              )}
          </div>
        </div>

        <div className={styles.componentHeaderRight}>
          {progressLabel ? (
            <span className={styles.componentProgress} role="status">
              <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />
              {progressLabel}
            </span>
          ) : (
            <div className={styles.componentState} aria-label={`${label} installed state`}>
              <StatusBadge tone={stateTone} variant={stateVariant}>
                {stateLabel}
              </StatusBadge>
            </div>
          )}
          {presentation.canRemove && (
            <IconButton
              className={styles.componentRemoveButton}
              tone="dangerQuiet"
              label={`Remove ${label}`}
              disabled={mutationBusy}
              onClick={onRemove}
            >
              <Trash2 size={iconSize.xs} aria-hidden="true" />
            </IconButton>
          )}
        </div>
      </div>

      {diagnostic && <p className={styles.componentDiagnostic}>{diagnostic}</p>}

      <div className={styles.componentCardFooter}>
        {presentation.primaryAction === "Retry inspection" ? (
          <ActionButton
            tone="secondary"
            className={styles.componentPrimaryAction}
            disabled={busy || mutationBusy}
            onClick={onRetryInspection}
          >
            <RefreshCw size={iconSize.xs} aria-hidden="true" />
            Retry inspection
          </ActionButton>
        ) : primaryAction && row.supports_version ? (
          <form
            className={`${styles.componentInputGroup} ${
              !isValid && trimmedVersion.length > 0 ? styles.componentInputGroupError : ""
            }`}
            onSubmit={handleInstallSubmit}
            title={validationError ?? undefined}
          >
            <div
              className={styles.componentVersionInputWrapper}
              onClick={() => inputRef.current?.focus()}
            >
              <span className={styles.componentVersionPrefix} aria-hidden="true">
                v
              </span>
              <input
                ref={inputRef}
                type="text"
                className={styles.componentVersionInput}
                value={effectiveVersion}
                placeholder={defaultTargetVersion || "X.Y.Z"}
                onChange={handleVersionChange}
                aria-label={`${label} version`}
                aria-invalid={!isValid && trimmedVersion.length > 0}
                disabled={mutationBusy}
              />
            </div>
            <ActionButton
              type="submit"
              tone="primarySoft"
              className={styles.componentInputGroupButton}
              disabled={!isValid || mutationBusy}
              title={validationError ?? `${primaryAction} ${label}`}
            >
              {primaryAction === "Update" ? (
                <ArrowUp size={iconSize.xs} aria-hidden="true" />
              ) : (
                <Download size={iconSize.xs} aria-hidden="true" />
              )}{" "}
              {primaryAction}
            </ActionButton>
          </form>
        ) : primaryAction ? (
          <ActionButton
            tone="primarySoft"
            className={styles.componentPrimaryAction}
            disabled={mutationBusy}
            onClick={() => onInstall(null)}
          >
            {primaryAction === "Update" ? (
              <ArrowUp size={iconSize.xs} aria-hidden="true" />
            ) : (
              <Download size={iconSize.xs} aria-hidden="true" />
            )}{" "}
            {primaryAction}
          </ActionButton>
        ) : row.status === "installed" || row.status === "modified" ? (
          <div className={styles.componentCardUpToDate}>
            <Check size={iconSize.xs} className={styles.versionCheckIcon} aria-hidden="true" />
            <span>Up to date</span>
          </div>
        ) : null}
      </div>
    </>
  );
}

/**
 * Standard Component card for Runtimes & Toolchains as well as Host Tenant Statuslines.
 */
export function ComponentRowItem(props: ComponentRowItemProps) {
  const rowRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!props.highlighted) return;
    const node = rowRef.current;
    if (!node || typeof node.scrollIntoView !== "function") return;
    const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches ?? false;
    node.scrollIntoView({ block: "center", behavior: reduceMotion ? "auto" : "smooth" });
  }, [props.highlighted]);

  return (
    <div
      ref={rowRef}
      className={`${styles.componentCard} ${props.progressLabel ? styles.componentCardBusy : ""}`}
      role="listitem"
      data-attention={props.highlighted ? "true" : undefined}
    >
      <ComponentCardInner {...props} />
    </div>
  );
}

/**
 * Bento-style Agent card: main Agent body on top, embedded Statusline companion sub-panel at bottom.
 */
export function ComponentAgentCard({ agent, statusline }: ComponentAgentCardProps) {
  const agentRowRef = useRef<HTMLDivElement>(null);
  const statuslineRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (agent.highlighted) {
      const node = agentRowRef.current;
      if (!node || typeof node.scrollIntoView !== "function") return;
      const reduceMotion =
        window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches ?? false;
      node.scrollIntoView({ block: "center", behavior: reduceMotion ? "auto" : "smooth" });
    } else if (statusline?.highlighted) {
      const node = statuslineRef.current;
      if (!node || typeof node.scrollIntoView !== "function") return;
      const reduceMotion =
        window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches ?? false;
      node.scrollIntoView({ block: "center", behavior: reduceMotion ? "auto" : "smooth" });
    }
  }, [agent.highlighted, statusline?.highlighted]);

  const statuslineTone: StatusTone = statusline
    ? statusline.row.error || !statusline.row.status
      ? "error"
      : statusline.row.status === "installed"
        ? "good"
        : statusline.row.status === "not-installed"
          ? "neutral"
          : "warning"
    : "neutral";
  const statuslineVariant: StatusVariant =
    statuslineTone === "good" || statuslineTone === "neutral" ? "inline" : "badge";

  return (
    <div className={styles.componentCard}>
      <div
        ref={agentRowRef}
        className={`${styles.componentCardMain} ${
          agent.progressLabel ? styles.componentCardBusy : ""
        }`}
        role="listitem"
        data-attention={agent.highlighted ? "true" : undefined}
      >
        <ComponentCardInner {...agent} />
      </div>

      {statusline && (
        <div
          ref={statuslineRef}
          role="listitem"
          className={`${styles.componentSubPanel} ${
            statusline.progressLabel ? styles.componentSubPanelBusy : ""
          }`}
          data-attention={statusline.highlighted ? "true" : undefined}
        >
          <div className={styles.componentSubPanelLeft}>
            <span
              className={styles.componentSubPanelIcon}
              data-component-icon={statusline.row.kind}
            >
              <ComponentGlyph kind={statusline.row.kind} />
            </span>
            <strong title={statusline.model.label}>{statusline.model.label}</strong>
          </div>
          <div className={styles.componentSubPanelActions}>
            {statusline.model.presentation.primaryAction === "Retry inspection" ? (
              <ActionButton
                tone="secondary"
                className={styles.componentSubPanelAction}
                disabled={statusline.busy || statusline.mutationBusy}
                onClick={statusline.onRetryInspection}
              >
                <RefreshCw size={iconSize.xs} aria-hidden="true" />
                Retry inspection
              </ActionButton>
            ) : statusline.model.primaryAction ? (
              <ActionButton
                tone="secondary"
                className={styles.componentSubPanelAction}
                disabled={statusline.mutationBusy}
                onClick={statusline.onInstall}
              >
                {statusline.model.primaryAction === "Update" ? (
                  <ArrowUp size={iconSize.xs} aria-hidden="true" />
                ) : (
                  <Download size={iconSize.xs} aria-hidden="true" />
                )}{" "}
                {statusline.model.primaryAction}
              </ActionButton>
            ) : null}
            <div
              className={styles.componentState}
              aria-label={`${statusline.model.label} installed state`}
            >
              {statusline.progressLabel ? (
                <span className={styles.componentProgress} role="status">
                  <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />
                  {statusline.progressLabel}
                </span>
              ) : (
                <StatusBadge tone={statuslineTone} variant={statuslineVariant}>
                  {statusline.model.presentation.stateBadge ??
                    statusline.model.presentation.stateLabel}
                </StatusBadge>
              )}
            </div>
            {statusline.model.presentation.canRemove && (
              <IconButton
                className={styles.componentRemoveButton}
                tone="dangerQuiet"
                label={`Remove ${statusline.model.label}`}
                disabled={statusline.mutationBusy}
                onClick={statusline.onRemove}
              >
                <Trash2 size={iconSize.xs} aria-hidden="true" />
              </IconButton>
            )}
          </div>
          {statusline.model.diagnostic && (
            <p className={styles.componentDiagnostic}>{statusline.model.diagnostic}</p>
          )}
        </div>
      )}
    </div>
  );
}
