import { useCallback, useEffect, useState } from "react";

export function useTestLocation(
  search: string | undefined,
  notify?: (query: URLSearchParams, replace?: boolean) => void,
) {
  const [currentSearch, setCurrentSearch] = useState(search ?? window.location.search);
  useEffect(() => {
    // Test wrappers mirror the shell's immutable route snapshot when rerendered.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (search !== undefined) setCurrentSearch(search);
  }, [search]);
  const onLocationChange = useCallback(
    (query: URLSearchParams, replace = false) => {
      const suffix = query.toString();
      const nextSearch = suffix ? `?${suffix}` : "";
      window.history[replace ? "replaceState" : "pushState"](
        null,
        "",
        `${window.location.pathname}${nextSearch}`,
      );
      setCurrentSearch(nextSearch);
      notify?.(query, replace);
    },
    [notify],
  );
  return { currentSearch, onLocationChange };
}
