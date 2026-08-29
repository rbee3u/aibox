import claudeIcon from "@/shared/icons/brand/claude.svg";
import githubIcon from "@/shared/icons/brand/github.svg";
import goIcon from "@/shared/icons/brand/go.svg";
import nodejsIcon from "@/shared/icons/brand/nodejs.svg";
import openaiIcon from "@/shared/icons/brand/openai.svg";
import pythonIcon from "@/shared/icons/brand/python.svg";
import rustIcon from "@/shared/icons/brand/rust.svg";
import type { CSSProperties } from "react";
import type { CodingAgentKind } from "@/domain/codingAgent";
import styles from "@/shared/icons/brandIcons.module.css";

export type BrandName = "github" | "openai" | "claude" | "nodejs" | "python" | "rust" | "go";

const brandIcons: Record<BrandName, string> = {
  claude: claudeIcon,
  github: githubIcon,
  go: goIcon,
  nodejs: nodejsIcon,
  openai: openaiIcon,
  python: pythonIcon,
  rust: rustIcon,
};

interface BrandIconProps {
  brand: BrandName;
  size: number;
  className?: string;
}

export function brandForAgent(agent: CodingAgentKind): BrandName {
  return agent === "codex" ? "openai" : "claude";
}

export function BrandIcon({ brand, size, className }: BrandIconProps) {
  return (
    <span
      aria-hidden="true"
      className={`${styles.brandIcon}${className ? ` ${className}` : ""}`}
      data-icon={brand}
      // The SVG data URLs contain single quotes. Quoting the CSS URL keeps
      // those characters inside the URL token instead of making the mask invalid.
      style={
        {
          "--brand-icon": `url("${brandIcons[brand]}")`,
          "--brand-icon-size": `${size}px`,
        } as CSSProperties
      }
    />
  );
}
