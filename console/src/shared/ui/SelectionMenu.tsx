import { Check, ChevronDown, ChevronLeft, ListChecks } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { ActionButton } from "@/shared/ui/ActionButton";
import styles from "@/shared/ui/SelectionMenu.module.css";

export interface SelectionOption<T extends string> {
  value: T;
  label: string;
  summaryLabel?: string;
  icon: ReactNode;
}
export function SelectionMenu<T extends string>({
  allowMultiple = true,
  className,
  disabled,
  label,
  onCommit,
  options,
  pluralLabel,
  selected,
  triggerIcon,
  unavailableSummary,
}: {
  allowMultiple?: boolean;
  className?: string;
  disabled: boolean;
  label: string;
  onCommit: (values: ReadonlySet<T>) => void;
  options: readonly SelectionOption<T>[];
  pluralLabel: string;
  selected: ReadonlySet<T>;
  triggerIcon: ReactNode;
  unavailableSummary?: string;
}) {
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<"single" | "multiple" | "choose-one">("single");
  const [draft, setDraft] = useState<Set<T>>(() => new Set(selected));
  const menuId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedOption = options.find((option) => selected.has(option.value));
  const summary =
    selected.size === 1
      ? (selectedOption?.summaryLabel ??
        selectedOption?.label ??
        unavailableSummary ??
        "1 selected")
      : `${selected.size} ${pluralLabel}`;
  const draftChanged =
    draft.size !== selected.size || [...draft].some((value) => !selected.has(value));
  const singleSelectedValue = selected.size === 1 ? [...selected][0] : undefined;
  const singleFocusIndex = Math.max(
    0,
    options.findIndex((option) => option.value === singleSelectedValue),
  );
  const multiFocusIndex = Math.max(
    0,
    options.findIndex((option) => draft.has(option.value)),
  );
  function openMenu() {
    setDraft(new Set(selected));
    setMode(allowMultiple && selected.size > 1 ? "multiple" : "single");
    setOpen(true);
  }
  useEffect(() => {
    if (!open) return;
    function closeOnOutsidePointer(event: PointerEvent) {
      if (rootRef.current?.contains(event.target as Node)) return;
      setOpen(false);
    }
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [open]);
  function closeAndFocusTrigger() {
    setOpen(false);
    triggerRef.current?.focus();
  }
  function commitOnly(value: T) {
    if (selected.size !== 1 || !selected.has(value)) onCommit(new Set([value]));
    closeAndFocusTrigger();
  }
  function toggleDraft(value: T) {
    setDraft((current) => {
      if (current.has(value) && current.size === 1) return current;
      const next = new Set(current);
      if (!next.delete(value)) next.add(value);
      return next;
    });
  }
  function applyDraft() {
    if (!draftChanged) return;
    onCommit(new Set(draft));
    closeAndFocusTrigger();
  }
  function handleSingleOptionKeyDown(event: ReactKeyboardEvent<HTMLButtonElement>, index: number) {
    let nextIndex: number | null = null;
    if (event.key === "ArrowDown") nextIndex = (index + 1) % options.length;
    if (event.key === "ArrowUp") nextIndex = (index - 1 + options.length) % options.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = options.length - 1;
    if (event.key === "Escape") {
      event.preventDefault();
      closeAndFocusTrigger();
      return;
    }
    if (nextIndex === null) return;
    event.preventDefault();
    optionRefs.current[nextIndex]?.focus();
  }
  return (
    <div
      ref={rootRef}
      className={`${styles.selection} ${className ?? ""}`}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
      }}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !open) return;
        event.preventDefault();
        closeAndFocusTrigger();
      }}
    >
      <button
        ref={triggerRef}
        type="button"
        className={styles.selectionTrigger}
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label={`${label}: ${summary}`}
        disabled={disabled}
        onClick={() => {
          if (open) setOpen(false);
          else openMenu();
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          if (!open) openMenu();
        }}
      >
        <span className={styles.selectionTriggerIcon}>{triggerIcon}</span>
        <span className={styles.selectionTriggerSummary}>
          {selected.size === 1 ? (
            summary
          ) : (
            <>
              <span className={styles.selectionSummaryFull}>{summary}</span>
              <span className={styles.selectionSummaryCompact} aria-hidden="true">
                {selected.size}
              </span>
            </>
          )}
        </span>
        <ChevronDown
          className={open ? styles.selectionChevronOpen : undefined}
          size={13}
          aria-hidden="true"
        />
      </button>
      {open && (
        <div id={menuId} className={styles.selectionMenu} role="dialog" aria-label={label}>
          {mode === "choose-one" && (
            <div className={styles.selectionMenuHeader}>
              <ActionButton
                tone="ghost"
                className={styles.selectionMenuAction}
                aria-label={`Back to multiple ${pluralLabel}`}
                onClick={() => setMode("multiple")}
              >
                <ChevronLeft size={13} aria-hidden="true" />
                Back
              </ActionButton>
            </div>
          )}
          {mode === "multiple" ? (
            <div className={styles.selectionOptions} role="group" aria-label={pluralLabel}>
              {options.map((option, index) => {
                const checked = draft.has(option.value);
                return (
                  <label
                    className={`${styles.selectionOption} ${styles.selectionOptionMultiple}`}
                    key={option.value}
                  >
                    <input
                      autoFocus={index === multiFocusIndex}
                      type="checkbox"
                      checked={checked}
                      disabled={checked && draft.size === 1}
                      onChange={() => toggleDraft(option.value)}
                    />
                    <span className={styles.selectionOptionIcon}>{option.icon}</span>
                    <span className={styles.selectionOptionLabel}>{option.label}</span>
                  </label>
                );
              })}
            </div>
          ) : (
            <div
              className={styles.selectionOptions}
              role="listbox"
              aria-label={`${label} single selection`}
            >
              {options.map((option, index) => {
                const active = mode === "single" && option.value === singleSelectedValue;
                return (
                  <button
                    autoFocus={index === singleFocusIndex}
                    type="button"
                    role="option"
                    aria-selected={active}
                    className={`${styles.selectionOption} ${styles.selectionOptionSingle} ${active ? styles.selectionOptionSelected : ""}`}
                    key={option.value}
                    ref={(element) => {
                      optionRefs.current[index] = element;
                    }}
                    onClick={() => commitOnly(option.value)}
                    onKeyDown={(event) => handleSingleOptionKeyDown(event, index)}
                  >
                    <span className={styles.selectionOptionIcon}>{option.icon}</span>
                    <span className={styles.selectionOptionLabel}>{option.label}</span>
                    <span className={styles.selectionOptionCheckSlot} aria-hidden="true">
                      {active && <Check className={styles.selectionOptionCheck} size={14} />}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
          {mode === "single" && allowMultiple && (
            <div className={styles.selectionMenuFooter}>
              <ActionButton
                tone="ghost"
                className={`${styles.selectionMenuAction} ${styles.selectionModeAction}`}
                aria-label={`Select multiple ${pluralLabel}`}
                onClick={() => {
                  setDraft(new Set(selected));
                  setMode("multiple");
                }}
              >
                <ListChecks size={13} aria-hidden="true" />
                Select multiple
              </ActionButton>
            </div>
          )}
          {mode === "multiple" && (
            <div className={`${styles.selectionMenuFooter} ${styles.selectionMenuFooterMultiple}`}>
              <ActionButton
                tone="ghost"
                className={`${styles.selectionMenuAction} ${styles.selectionModeAction}`}
                aria-label={`Choose one ${label}`}
                onClick={() => setMode("choose-one")}
              >
                Choose one
              </ActionButton>
              <div className={styles.selectionCommitActions}>
                <ActionButton
                  tone="ghost"
                  className={styles.selectionMenuAction}
                  onClick={closeAndFocusTrigger}
                >
                  Cancel
                </ActionButton>
                <ActionButton
                  tone="secondary"
                  className={styles.selectionMenuAction}
                  disabled={!draftChanged}
                  onClick={applyDraft}
                >
                  Apply
                </ActionButton>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
