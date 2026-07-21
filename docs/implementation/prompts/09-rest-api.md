# Prompt 09: REST and Streaming API

## Objective

Expose health, tool discovery state, native sessions, and events through a
versioned localhost API with pagination, filters, and SSE.

## Prerequisites

Prompts 04 and 08 are complete.

## Work

- Implement the Milestone 1 subset of `/api/v1/health`, `/tools`,
  `/tools/discover`, `/native-sessions`, native-session detail, and event
  streaming.
- Define stable response, pagination, filter, ordering, and error envelopes.
- Bind to `127.0.0.1` by default; introduce the local auth boundary before any
  state-changing production endpoint is enabled.
- Prevent raw secret-bearing payloads from appearing in list endpoints.
- Add SSE resume identifiers and bounded client buffering.

## TDD and acceptance

- Start with handler contract tests, then storage-backed integration tests.
- Cover invalid cursors, stable page ordering, empty results, parser errors,
  auth failure, slow/disconnected SSE clients, and restart recovery.
- Generate or maintain API documentation and examples.
- Pass all gates, update the ledger, and select Prompt 10.
