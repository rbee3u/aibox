/**
 * The editor pane publishes this handle so the page can save, restore, or
 * reload one Config file without owning its editing state.
 */
export interface ConfigFileController {
  dirty: boolean;
  canSave: boolean;
  save: () => Promise<boolean>;
  restore: () => void;
  reload: () => void;
}
