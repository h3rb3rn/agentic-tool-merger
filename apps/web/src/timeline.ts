import type { EventSummary } from "./api";

/**
 * Merges initial, replayed, and live events by deterministic identity and
 * restores the chronology used by the server.
 */
export function mergeChronologicalEvents(
  current: EventSummary[],
  incoming: EventSummary[],
): EventSummary[] {
  const unique = new Map(current.map((event) => [event.event_id, event]));
  for (const event of incoming) unique.set(event.event_id, event);
  return [...unique.values()].sort((left, right) => {
    const timestamp = Date.parse(left.timestamp) - Date.parse(right.timestamp);
    if (Number.isFinite(timestamp) && timestamp !== 0) return timestamp;
    if (left.sequence !== right.sequence) return left.sequence - right.sequence;
    return left.event_id.localeCompare(right.event_id);
  });
}

export function eventLabel(kind: string): string {
  if (!kind) return "Unknown event";
  return kind
    .split("_")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}
