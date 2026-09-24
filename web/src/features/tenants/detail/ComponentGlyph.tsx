import { Activity } from "lucide-react";
import type { ComponentKind } from "@/api/tenants";
import { COMPONENT_BRANDS, isStatuslineComponent } from "@/features/tenants/componentCatalog";
import { BrandIcon } from "@/shared/icons/brandIcons";
import { iconSize } from "@/shared/icons/iconSizes";

/** Bare Component icon; both statuslines share the waveform glyph. */
export function ComponentGlyph({ kind }: { kind: ComponentKind }) {
  if (isStatuslineComponent(kind)) {
    return <Activity size={iconSize.lg} aria-hidden="true" />;
  }
  return <BrandIcon brand={COMPONENT_BRANDS[kind]} size={iconSize.lg} />;
}
