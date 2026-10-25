import { EVENT_TYPES } from "@/api/types";

/** Event-type patterns offered as one-click additions. */
export const EVENT_SUGGESTIONS: readonly string[] = [
  "*",
  ...EVENT_TYPES,
  "branch.deleted",
  "tag.deleted",
  "mr.*",
  "mr.reviewed",
  "mr.commented",
  "ci.status",
  "todo.created",
  "todo.updated",
  "release.created",
  "release.deleted",
  "group.merged",
  "access.applied",
  "policy.changed",
  "webhook.failing",
];

const EVENT_RE = /^(\*|[a-z_]+(\.([a-z_]+|\*))*)$/;

/** Validation messages for the webhook form ("" = valid). */
export function webhookFormErrors(url: string, events: readonly string[]): { url: string; events: string } {
  const u = url.trim();
  let urlError = "";
  if (!u) urlError = "Required.";
  else {
    try {
      const parsed = new URL(u);
      if (parsed.protocol !== "https:" && parsed.protocol !== "http:") urlError = "Use an http:// or https:// URL.";
    } catch {
      urlError = "Enter a full URL, e.g. https://ci.example.com/hooks/wt.";
    }
  }
  const bad = events.find((e) => !EVENT_RE.test(e));
  const eventsError = events.length === 0 ? "Add at least one event (or * for every event)." : bad ? `“${bad}” is not an event pattern.` : "";
  return { url: urlError, events: eventsError };
}
