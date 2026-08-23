import type {
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from "react";
import { forwardRef } from "react";
import styles from "./FormControls.module.css";

export const TextInput = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function TextInput({ className, ...props }, ref) {
    return <input {...props} ref={ref} className={`${styles.control} ${className ?? ""}`} />;
  },
);

export const TextArea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement>
>(function TextArea({ className, ...props }, ref) {
  return <textarea {...props} ref={ref} className={`${styles.control} ${className ?? ""}`} />;
});

export function Toggle({
  onCheckedChange,
  className,
  children,
  ...props
}: Omit<InputHTMLAttributes<HTMLInputElement>, "className" | "onChange" | "type"> & {
  className?: string;
  children?: ReactNode;
  onCheckedChange?: (checked: boolean) => void;
}) {
  return (
    <label className={`${styles.toggle} ${className ?? ""}`}>
      <input
        {...props}
        type="checkbox"
        onChange={(event) => onCheckedChange?.(event.target.checked)}
      />
      <span className={styles.toggleMark} aria-hidden="true" />
      {children}
    </label>
  );
}

export function NativeSelect({ className, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} className={`${styles.control} ${className ?? ""}`} />;
}
