import type { ComponentProps } from "react";
import { modulePath, type ConsoleNavigate, type ModuleId } from "@/shared/lib/navigation";

/** Keep native link behavior for new tabs; ordinary clicks use the shell router. */
export function OverviewLink({
  targetModule,
  query,
  onNavigate,
  ...props
}: Omit<ComponentProps<"a">, "href"> & {
  targetModule: ModuleId;
  query?: URLSearchParams;
  onNavigate: ConsoleNavigate;
}) {
  return (
    <a
      {...props}
      href={modulePath(targetModule, query)}
      onClick={(event) => {
        props.onClick?.(event);
        if (
          event.defaultPrevented ||
          event.button !== 0 ||
          event.metaKey ||
          event.ctrlKey ||
          event.shiftKey ||
          event.altKey
        )
          return;
        event.preventDefault();
        onNavigate(targetModule, query);
      }}
    />
  );
}
