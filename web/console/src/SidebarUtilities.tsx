import anthropicIcon from "@lobehub/icons-static-svg/icons/anthropic.svg";
import githubIcon from "@lobehub/icons-static-svg/icons/github.svg";
import openaiIcon from "@lobehub/icons-static-svg/icons/openai.svg";
import {
  Check,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Monitor,
  Moon,
  Sun,
  type LucideIcon,
} from "lucide-react";
import {
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import type { ThemePreference } from "./usePersistentTheme";
import styles from "./App.module.css";

interface SidebarUtilitiesProps {
  collapsed: boolean;
  onThemeChange: (theme: ThemePreference) => void;
  onToggleCollapsed: () => void;
  theme: ThemePreference;
  version: string;
}

interface ThemeOption {
  icon: LucideIcon;
  label: string;
  value: ThemePreference;
}

const resources = [
  {
    href: "https://github.com/rbee3u/aibox",
    icon: githubIcon,
    label: "GitHub",
  },
  {
    href: "https://developers.openai.com/codex/cli",
    icon: openaiIcon,
    label: "Codex docs",
  },
  {
    href: "https://code.claude.com/docs/en/overview",
    icon: anthropicIcon,
    label: "Claude docs",
  },
] as const;

const themeOptions: readonly ThemeOption[] = [
  { icon: Monitor, label: "System", value: "system" },
  { icon: Sun, label: "Light", value: "light" },
  { icon: Moon, label: "Dark", value: "dark" },
];

export function SidebarUtilities({
  collapsed,
  onThemeChange,
  onToggleCollapsed,
  theme,
  version,
}: SidebarUtilitiesProps) {
  return (
    <footer className={styles.sidebarUtilities}>
      <nav className={styles.resourceLinks} aria-label="Resources">
        {resources.map((resource) => (
          <a
            className={styles.utilityItem}
            href={resource.href}
            key={resource.label}
            aria-label={resource.label}
            target="_blank"
            rel="noopener noreferrer"
            title={collapsed ? resource.label : undefined}
          >
            <BrandIcon source={resource.icon} />
            <span className={styles.utilityLabel}>{resource.label}</span>
          </a>
        ))}
      </nav>
      <div className={styles.utilityDivider} />
      <ThemeMenu collapsed={collapsed} onChange={onThemeChange} value={theme} />
      <div className={styles.sidebarMeta}>
        <span className={styles.sidebarVersion}>v{version}</span>
        <button
          className={styles.collapseButton}
          type="button"
          title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          onClick={onToggleCollapsed}
        >
          {collapsed ? (
            <ChevronRight size={17} aria-hidden="true" />
          ) : (
            <ChevronLeft size={17} aria-hidden="true" />
          )}
        </button>
      </div>
    </footer>
  );
}

function BrandIcon({ source }: { source: string }) {
  return (
    <span
      aria-hidden="true"
      className={styles.brandIcon}
      // The SVG data URLs contain single quotes. Quoting the CSS URL keeps
      // those characters inside the URL token instead of making the mask
      // declaration invalid in the browser.
      style={{ "--brand-icon": `url("${source}")` } as CSSProperties}
    />
  );
}

function ThemeMenu({
  collapsed,
  onChange,
  value,
}: {
  collapsed: boolean;
  onChange: (theme: ThemePreference) => void;
  value: ThemePreference;
}) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<CSSProperties>({ visibility: "hidden" });
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const activeIndex = themeOptions.findIndex((option) => option.value === value);
  const activeOption = themeOptions[activeIndex];
  const ActiveIcon = activeOption.icon;

  useLayoutEffect(() => {
    if (!open) return;

    function placeMenu() {
      const trigger = triggerRef.current;
      const menu = menuRef.current;
      if (!trigger || !menu) return;

      const margin = 8;
      const gap = 6;
      const triggerRect = trigger.getBoundingClientRect();
      const menuRect = menu.getBoundingClientRect();
      const narrowLayout = window.innerWidth <= 900;
      const preferredLeft = collapsed && !narrowLayout ? triggerRect.right + gap : triggerRect.left;
      const left = Math.min(
        Math.max(margin, preferredLeft),
        Math.max(margin, window.innerWidth - menuRect.width - margin),
      );
      const preferredTop = triggerRect.top - menuRect.height - gap;
      const belowTop = triggerRect.bottom + gap;
      const top =
        preferredTop >= margin
          ? preferredTop
          : Math.min(belowTop, Math.max(margin, window.innerHeight - menuRect.height - margin));

      setPosition({ left, top });
    }

    placeMenu();
    window.addEventListener("resize", placeMenu);
    return () => window.removeEventListener("resize", placeMenu);
  }, [collapsed, open]);

  useEffect(() => {
    if (!open) return;

    itemRefs.current[activeIndex]?.focus();

    function closeOnOutsidePointer(event: PointerEvent) {
      const target = event.target as Node;
      if (menuRef.current?.contains(target) || triggerRef.current?.contains(target)) return;
      setOpen(false);
    }

    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [activeIndex, open]);

  function closeAndFocusTrigger() {
    setOpen(false);
    triggerRef.current?.focus();
  }

  function selectTheme(nextTheme: ThemePreference) {
    onChange(nextTheme);
    closeAndFocusTrigger();
  }

  function handleTriggerKeyDown(event: ReactKeyboardEvent<HTMLButtonElement>) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    setOpen(true);
  }

  function handleItemKeyDown(event: ReactKeyboardEvent<HTMLButtonElement>, index: number) {
    let nextIndex: number | null = null;
    if (event.key === "ArrowDown") nextIndex = (index + 1) % themeOptions.length;
    if (event.key === "ArrowUp")
      nextIndex = (index - 1 + themeOptions.length) % themeOptions.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = themeOptions.length - 1;
    if (event.key === "Escape") {
      event.preventDefault();
      closeAndFocusTrigger();
      return;
    }
    if (event.key === "Tab") {
      setOpen(false);
      return;
    }
    if (nextIndex === null) return;
    event.preventDefault();
    itemRefs.current[nextIndex]?.focus();
  }

  return (
    <>
      <button
        className={`${styles.utilityItem} ${styles.themeButton}`}
        type="button"
        ref={triggerRef}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`Color theme: ${activeOption.label}`}
        title={collapsed ? `Color theme: ${activeOption.label}` : undefined}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={handleTriggerKeyDown}
      >
        <ActiveIcon size={17} aria-hidden="true" />
        <span className={styles.utilityLabel}>Theme</span>
        <span className={styles.themeValue}>{activeOption.label}</span>
        <ChevronUp
          className={`${styles.themeChevron} ${open ? styles.themeChevronOpen : ""}`}
          size={14}
          aria-hidden="true"
        />
      </button>
      {open &&
        createPortal(
          <div
            className={styles.themeMenu}
            ref={menuRef}
            role="menu"
            aria-label="Color theme"
            style={position}
          >
            {themeOptions.map((option, index) => {
              const Icon = option.icon;
              const selected = option.value === value;
              return (
                <button
                  className={styles.themeOption}
                  key={option.value}
                  type="button"
                  role="menuitemradio"
                  aria-checked={selected}
                  ref={(element) => {
                    itemRefs.current[index] = element;
                  }}
                  tabIndex={selected ? 0 : -1}
                  onClick={() => selectTheme(option.value)}
                  onKeyDown={(event) => handleItemKeyDown(event, index)}
                >
                  <Icon size={17} aria-hidden="true" />
                  <span>{option.label}</span>
                  {selected && <Check className={styles.themeCheck} size={15} aria-hidden="true" />}
                </button>
              );
            })}
          </div>,
          document.body,
        )}
    </>
  );
}
