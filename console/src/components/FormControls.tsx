import { Checkbox, Input } from "antd";
import type { CheckboxProps, InputRef } from "antd";
import type { InputProps, TextAreaProps } from "antd/es/input";
import type { SelectHTMLAttributes } from "react";
import { forwardRef } from "react";

export const TextInput = forwardRef<HTMLInputElement, InputProps>(function TextInput(props, ref) {
  return (
    <Input
      {...props}
      data-aibox-control="input"
      ref={(instance: InputRef | null) => {
        const element = instance?.input ?? null;
        if (typeof ref === "function") ref(element);
        else if (ref) ref.current = element;
      }}
    />
  );
});

export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(
  function TextArea(props, ref) {
    return (
      <Input.TextArea
        {...props}
        data-aibox-control="textarea"
        ref={(instance) => {
          const element = instance?.resizableTextArea?.textArea ?? null;
          if (typeof ref === "function") ref(element);
          else if (ref) ref.current = element;
        }}
      />
    );
  },
);

export function Toggle({
  onCheckedChange,
  ...props
}: Omit<CheckboxProps, "onChange"> & { onCheckedChange?: (checked: boolean) => void }) {
  return (
    <Checkbox
      {...props}
      data-aibox-control="checkbox"
      onChange={(event) => onCheckedChange?.(event.target.checked)}
    />
  );
}

export function NativeSelect(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} data-aibox-control="select" />;
}
