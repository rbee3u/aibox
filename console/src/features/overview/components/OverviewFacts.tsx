import { AlertTriangle } from "lucide-react";
import type { ReactNode } from "react";
import type { Tone } from "@/features/overview/viewTypes";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import styles from "@/features/overview/OverviewPage.module.css";

interface FactProps {
  icon: ReactNode;
  label: string;
  value: ReactNode;
  detail: string;
  tone: Tone;
  /** A navigable fact renders as a button and opens its module. */
  onClick?: () => void;
}

export function Fact({ icon, label, value, detail, tone, onClick }: FactProps) {
  const content = (
    <>
      <span className={styles.factLabel}>
        {icon}
        {label}
      </span>
      <strong>{value}</strong>
      <small title={detail}>{detail}</small>
    </>
  );
  return onClick ? (
    <button
      type="button"
      className={`${styles.fact} ${styles[tone]}`}
      data-overview-fact
      onClick={onClick}
    >
      {content}
    </button>
  ) : (
    <div className={`${styles.fact} ${styles[tone]}`} data-overview-fact>
      {content}
    </div>
  );
}

interface MetadataProps {
  icon: ReactNode;
  label: string;
  value: string;
  mono?: boolean;
  wide?: boolean;
}

export function Metadata({ icon, label, value, mono = false, wide = false }: MetadataProps) {
  return (
    <div className={`${styles.metadata} ${wide ? styles.metadataWide : ""}`}>
      {icon}
      <span>{label}</span>
      <code className={mono ? styles.mono : undefined} title={value}>
        {value}
      </code>
    </div>
  );
}

interface RuntimeStatusProps {
  icon: ReactNode;
  label: string;
  value: string;
  detail: string;
  tone: Tone;
}

export function RuntimeStatus({ icon, label, value, detail, tone }: RuntimeStatusProps) {
  return (
    <div className={styles.runtimeStatus}>
      <span className={`${styles.statusIcon} ${styles[tone]}`}>{icon}</span>
      <div>
        <span>{label}</span>
        <strong>{value}</strong>
        <small title={detail}>{detail}</small>
      </div>
    </div>
  );
}

/** Page-level failures use the shell banner; local ones stay inside a section. */
export function ErrorBanner({ message, local = false }: { message: string; local?: boolean }) {
  return (
    <AlertBanner
      className={local ? styles.localError : styles.errorBanner}
      icon={<AlertTriangle size={16} />}
    >
      {message}
    </AlertBanner>
  );
}
