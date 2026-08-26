import { act, fireEvent, render, screen } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";
import { useCallback, useEffect, useState } from "react";
import { afterEach, expect, vi } from "vitest";
import type { RequestsApi } from "@/api/requests";
import { RequestsPage } from "@/features/requests/RequestsPage";
import { requestsApiFake } from "@/features/requests/testFixtures";

type User = ReturnType<typeof userEvent.setup>;

export const zstdBytes = new Uint8Array([0x28, 0xb5, 0x2f, 0xfd]);

/** Stands in for the App shell: it owns history and feeds the page its search. */
export function RequestsHarness({ api }: { api: RequestsApi }) {
  const [search, setSearch] = useState(window.location.search);
  useEffect(() => {
    const readLocation = () => setSearch(window.location.search);
    window.addEventListener("popstate", readLocation);
    return () => window.removeEventListener("popstate", readLocation);
  }, []);
  const onLocationChange = useCallback((query: URLSearchParams, replace = false) => {
    const suffix = query.toString();
    const next = `${window.location.pathname}${suffix ? `?${suffix}` : ""}`;
    window.history[replace ? "replaceState" : "pushState"](null, "", next);
    setSearch(suffix ? `?${suffix}` : "");
  }, []);
  return <RequestsPage api={api} search={search} onLocationChange={onLocationChange} />;
}

export function renderApp(overrides: Partial<RequestsApi> = {}) {
  return render(<RequestsHarness api={requestsApiFake(overrides)} />);
}

export function flushEffects() {
  return act(async () => Promise.resolve());
}

export function advanceTimers(milliseconds: number) {
  return act(async () => vi.advanceTimersByTimeAsync(milliseconds));
}

export async function selectCompletedRecord(user: User) {
  await user.click(await screen.findByRole("button", { name: "Select Requests" }));
  await user.click(
    screen.getByRole("button", { name: "Select POST api.example.test/v1/responses" }),
  );
}

export async function openCompletedRecord(user: User) {
  await user.click(
    await screen.findByRole("button", { name: "POST api.example.test/v1/responses" }),
  );
}

export async function openActiveRecord() {
  await flushEffects();
  fireEvent.click(screen.getByRole("button", { name: "GET stream.example.test/events" }));
  await flushEffects();
}

export async function openActiveRequestBody() {
  await openActiveRecord();
  fireEvent.click(screen.getByRole("tab", { name: "Request" }));
  await flushEffects();
}

export async function confirmDeletion(user: User, action: "Delete selected") {
  await user.click(screen.getByRole("button", { name: action }));
  await user.click(screen.getByRole("button", { name: "Delete permanently" }));
}

export async function confirmSingleDeletion(user: User, buttonName: string) {
  await user.click(await screen.findByRole("button", { name: buttonName }));
  expect(screen.getByRole("dialog", { name: "Delete this Request?" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Delete permanently" }));
}

afterEach(() => {
  vi.useRealTimers();
  window.history.replaceState(null, "", "/");
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});
