# ADR-009: Chronology Uses Multiple Ordering Signals

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

Timestamps from separate tools may have different precision, identical values,
clock skew, delayed writes, or missing fields. Compaction and resumed sessions
can also record events later than the work they describe.

## Decision

Preserve each source's native order and derive global chronology with an
explicit, deterministic ordering key:

```text
effective_timestamp
native_sequence
source_generation
source_byte_offset
ingestion_sequence
event_id
```

Source-local sequence and offsets define order within a source. Timestamps are
the primary cross-source signal. Remaining fields resolve ties deterministically.
The event model records original timestamp, timestamp provenance, precision,
ingestion time, and ordering confidence separately.

Late events may change a derived global view but never mutate original source
order or provenance. Relationships with stronger causal evidence, such as an
explicit handoff ID or parent event, are stored separately from display order.

## Consequences

The same event set produces the same timeline. The UI can distinguish exact
source order from inferred cross-source order and can flag uncertainty.
Timestamp-only sorting is prohibited in repositories, APIs, and clients.
