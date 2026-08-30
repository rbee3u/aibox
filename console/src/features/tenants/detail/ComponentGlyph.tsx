import { Activity } from "lucide-react";
import type { ComponentKind } from "@/api/tenants";
import { COMPONENT_BRANDS, isStatuslineComponent } from "@/features/tenants/componentCatalog";
import { BrandIcon } from "@/shared/icons/brandIcons";

/** Bare Component icon; both statuslines share the waveform glyph. */
export function ComponentGlyph({ kind }: { kind: ComponentKind }) {
  if (isStatuslineComponent(kind)) {
    return <Activity size={24} strokeWidth={1.8} aria-hidden="true" />;
  }
  return <BrandIcon brand={COMPONENT_BRANDS[kind]} size={24} />;
}
