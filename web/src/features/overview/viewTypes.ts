/**
 * Types more than one Overview concern shares.
 *
 * Topology and Component facts share this feature-local tone vocabulary.
 * Other Console surfaces define tones for different UI semantics.
 */
export type Tone = "good" | "neutral" | "warning" | "error";
/**
 * What the attention panel is currently showing.
 *
 * The health logic in `topology/` decides it and the panel in `components/`
 * renders it, so the type belongs to neither concern.
 */
export type AttentionPanelKind = "items" | "pending" | "healthy";
