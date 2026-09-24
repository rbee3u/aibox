import { routes } from "@/api/generated/routes";

export type ControlRouteKey = keyof typeof routes;

export function controlRoute(
  key: ControlRouteKey,
  parameters: Readonly<Record<string, string>> = {},
  query?: string,
): string {
  let path: string = routes[key].path;
  for (const [name, value] of Object.entries(parameters)) {
    path = path.replace(`{${name}}`, encodeURIComponent(value));
  }
  if (path.includes("{")) throw new Error(`Missing route parameter for ${key}: ${path}`);
  return query ? `${path}?${query}` : path;
}

export function controlMethod(key: ControlRouteKey): "GET" | "POST" {
  return routes[key].method;
}
