# Event Inspector design

Status: the public bounded live-summary feed, lazy normalized canonical-field
detail, pinning, filtering, true frozen review mode, dedicated native window,
privacy-gated local protobuf detail, and disabled local trigger drafting are
implemented. Allowlisted decoded messages also appear as explicit protocol
rows even when they produce no canonical event. Trigger execution is not
enabled yet.

## Purpose

The Event Inspector helps a user discover which live BPSR event should drive a
Custom Trigger. It follows the useful ACT/Triggernometry workflow of watching
events happen in real time, but adds structured protobuf inspection and a
direct **Create Trigger** path.

It replaces neither the Combat Meter nor the hidden developer Session
Recorder. It is a focused public tool inside Custom Triggers.

## User workflow

1. Open **Custom Triggers -> Event Inspector**. rLogs opens or restores the
   dedicated Event Inspector window.
2. Start Live Follow or inspect an already acknowledged local event stream.
3. Filter by route, event kind, source, target, skill, effect, scene, or changed
   fields.
4. Perform the action in game.
5. Freeze the log. This stops Inspector polling and retention while preserving
   the visible rows, pins, selected fields, and draft for review. Canonical
   capture continues independently.
6. Expand canonical fields or the local protocol/protobuf detail tree.
7. Pin one or more rows and compare field changes when necessary.
8. Select an event or field and choose **Create Trigger**.
9. Continue in the Rules editor with a localized, typed When clause already
   filled in.

Raw numeric IDs and protobuf tags remain visible beside localized names because
they are necessary evidence, but the generated rule prefers stable typed event
fields over display text.

## Three inspection levels

### 1. Summary

The high-rate list distinguishes canonical `E#` rows from protocol `P#` rows
and shows only sequence, observed time, route/event kind, compact identifiers,
and a one-line summary. Source, topic, kind, route, and raw IDs can be filtered
without materializing packet payloads. It never renders the entire retained
stream at once.

### 2. Canonical fields

The selected row shows the already-decoded, privacy-reviewed canonical event,
its provenance, build and protocol-pack identity, confidence, and gap evidence.
This is the default source for building triggers.

### 3. Protocol details

Advanced, local-only inspection shows the selected allowlisted framed
message's route, opcode/method identity, direction, payload length, and a
bounded generic protobuf wire tree:

- route/message name from the build-locked protocol pack;
- protobuf field number, repeated occurrence index, and wire type;
- exact decimal and optional hexadecimal representation;
- bounded text/hex previews for length-delimited values;
- unknown field number and wire value without inventing a semantic name.

The generic reader deliberately does not guess nested messages or field names.
Those appear only after a descriptor or canonical promotion provides
build-locked proof.

Protocol details are for discovery. A shared trigger pack never receives raw
packet access. Authentication, login, account secrets, private chat, encrypted
payloads, and unreviewed sensitive routes are rejected before this view.

## Bounded-memory design

Memory is controlled by exact byte budgets, not only event counts. The
implemented pipeline is:

```text
trusted capture/decoder
        |
        +--> canonical writer (authoritative, unchanged)
        |
        +--> bounded metadata ring --> filtered live batches --> virtualized UI
                    |
                    +--> small explicit pin store
                    +--> separate allowlisted protocol ring
                              |
                              +--> lazy selected-message wire decode
```

Requirements:

- the inspector references the writer's acknowledged events instead of cloning
  the complete history;
- the canonical rolling ring is capped at 8,192 rows and 4 MiB;
- the separate protocol ring is capped at 512 records and 2 MiB;
- an individual application payload is never copied when it exceeds 64 KiB;
- opaque, unrouted, prohibited, and non-gameplay messages never enter the
  protocol ring;
- old unpinned rows are overwritten in constant space;
- pinned rows have their own small byte/count quota and visible usage;
- protobuf fields are materialized only when a retained row is selected, with
  a 1,024-field ceiling and bounded value previews;
- repeated high-rate rows may be coalesced for display, while occurrence count,
  first/last sequence, and gap evidence stay visible;
- live batches are bounded and acknowledged; a slow UI drops display rows, not
  capture or authoritative canonical events;
- overflow counters distinguish overwritten inspector rows, coalesced display
  rows, decoder failures, and actual capture gaps;
- freezing cancels the Inspector subscription and lets its short reader lease
  expire; the visible browser working set remains unchanged for review, while
  the parser and canonical writer continue independently;
- search and filtering occur before expensive UI serialization where possible;
- no unbounded DOM nodes, strings, JSON copies, queues, or background tasks are
  allowed.

The inspector's display worker must be lower priority than capture, canonical
recording, encounter reduction, and the live Combat Meter. A saturated
inspector may become less visually granular, but it must not delay those paths.

## Recording policy

Continuous raw/protocol recording is off by default. If an explicit local
research recording option is approved later, it must require:

- a destination chosen by the user;
- a maximum bytes or duration limit before starting;
- bounded compressed chunks written incrementally rather than retained in RAM;
- free-space checks and a hard stop before the limit;
- visible recording state and one-click stop;
- a manifest of excluded sensitive routes and redactions;
- no automatic upload, profile inclusion, or trigger-pack inclusion.

Ordinary trigger creation needs no raw recording: the typed canonical event,
selected field path, build identity, and a small sanitized example are enough.

## Trigger creation contract

**Create Trigger** copies a typed selector, not the whole packet. The draft
contains:

- canonical event kind and schema version;
- field path and comparison selected by the user;
- raw stable ID plus localization key where applicable;
- required game build/protocol-pack compatibility if the field is build-bound;
- a sanitized example value;
- source evidence link to the local sequence, excluded from shared exports.

If only an unknown protobuf field is available, the rule is marked Advanced,
build-bound, local-only by default, and visibly unverified until the field is
promoted into the reviewed canonical schema.

## Menu placement

The Custom Triggers top-level tabs become:

1. Overview
2. Rules
3. Event Inspector
4. Library
5. Settings

The Event Inspector page contains Live Stream, Filters, Decoded Details, Pin &
Compare, Create Trigger, Memory & Recording, and Privacy Boundary sections.
