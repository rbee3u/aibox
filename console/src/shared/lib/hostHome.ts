/** Display a host-absolute path as `~` when it is the Host Home or sits under it. */
export function abbreviateTenantHome(path: string, hostHome: string | null): string {
  if (!hostHome) return path;
  if (path === hostHome) return "~";
  const prefix = hostHome.endsWith("/") ? hostHome : `${hostHome}/`;
  return path.startsWith(prefix) ? `~/${path.slice(prefix.length)}` : path;
}
