import claudeIcon from "@lobehub/icons-static-svg/icons/claude.svg";
import openaiIcon from "@lobehub/icons-static-svg/icons/openai.svg";
import type { CSSProperties } from "react";
import styles from "./icons.module.css";

export type AgentName = "claude" | "codex";

const agentIcons: Record<AgentName, string> = {
  claude: claudeIcon,
  codex: openaiIcon,
};

interface BrandIconProps {
  source: string;
  size?: number;
  name?: string;
  className?: string;
}

export function BrandIcon({ source, size = 17, name = "brand", className }: BrandIconProps) {
  return (
    <span
      aria-hidden="true"
      className={`${styles.brandIcon}${className ? ` ${className}` : ""}`}
      data-icon={name}
      // The SVG data URLs contain single quotes. Quoting the CSS URL keeps
      // those characters inside the URL token instead of making the mask invalid.
      style={
        {
          "--brand-icon": `url("${source}")`,
          "--brand-icon-size": `${size}px`,
        } as CSSProperties
      }
    />
  );
}

export function AgentIcon({ agent, size = 16 }: { agent: AgentName; size?: number }) {
  return <BrandIcon source={agentIcons[agent]} name={agent} size={size} />;
}
