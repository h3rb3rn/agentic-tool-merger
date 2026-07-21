# Prompt 10: Web Timeline

## Objective

Deliver the Codex vertical slice in the browser: native-session list,
chronological timeline, tool details, controlled raw-event view, filters, and
live updates.

## Prerequisites

Prompt 09 is complete.

## Work

- Add typed API access with TanStack Query and route-safe session identifiers.
- Build accessible loading, empty, error, and stale-data states.
- Render event kinds distinctly and group tool calls with results without
  losing chronology or provenance.
- Gate raw-event detail behind an explicit reveal and warn that it may contain
  secrets.
- Apply SSE updates idempotently without duplicating or reordering events.

## TDD and acceptance

- Write component and behavior tests before implementation.
- Cover keyboard navigation, filters, unknown events, long output, malformed
  data, reconnects, duplicate SSE events, and responsive layouts.
- An incrementally imported Codex event appears within seconds without reload.
- Document the UI and privacy boundary, pass all gates, and select Prompt 11.
