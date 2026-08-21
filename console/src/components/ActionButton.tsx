import { Button } from "antd";
import type { ButtonProps } from "antd";
import { forwardRef } from "react";

export type ActionButtonTone = "default" | "primary" | "danger" | "quiet";

export interface ActionButtonProps extends Omit<ButtonProps, "danger" | "htmlType" | "type"> {
  htmlType?: "button" | "submit" | "reset";
  tone?: ActionButtonTone;
}

export const ActionButton = forwardRef<HTMLButtonElement, ActionButtonProps>(function ActionButton(
  { htmlType = "button", tone = "default", className, children, ...props },
  ref,
) {
  return (
    <Button
      {...props}
      ref={ref}
      className={className}
      data-aibox-control="button"
      htmlType={htmlType}
      type={
        tone === "primary" || tone === "danger" ? "primary" : tone === "quiet" ? "text" : "default"
      }
      danger={tone === "danger"}
    >
      {children}
    </Button>
  );
});
