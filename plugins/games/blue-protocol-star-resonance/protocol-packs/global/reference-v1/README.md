# Global reference pack v1

This pack exercises the human-readable protocol-pack format and the first
selective decoders. Its target build is deliberately named
`reference-not-for-live-capture`; RLogs will not select it for a real client.

Mappings are historical/current-reference hypotheses from pinned parser
audits. Copying this file to a real build ID is not verification. Each route
must be replayed against a sanitized capture from that exact build first.

Opaque entries are coverage requirements, not discarded packets. Their wire
records remain in the local journal until a privacy review and typed decoder
are complete.
