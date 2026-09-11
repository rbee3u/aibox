/** Where the reader had scrolled Overview to, restored on return to the module. */
export interface OverviewBrowsingState {
  scrollTop: number;
}
export interface OverviewBrowsingMemory {
  current: OverviewBrowsingState | null;
}
