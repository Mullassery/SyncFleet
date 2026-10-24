# Queue status — 2026-10-19

This file records what was shipped, what is blocked/failed, and what is rejected for this repository. It complements `AUDIT_FINDINGS.md`.

## Shipped
- Conflict detection, remote/local/LWW policies, schema guard, manual schema update (0.7.0, published).
- Field-level `merge_fields` conflict policy (deterministic LWW-register over JSON fields, overlaps marked `merged`).

## Blocked / failed
- mDNS interface binding, relay/NAT. (Full cr-sqlite CRDT tables remain a separate optional integration.)

## Rejected / deferred
- A message broker.

## Policy

Any future feature or upgrade must check `AUDIT_FINDINGS.md` and this file before implementation. A blocked item is retried only when its external dependency is available; a rejected item needs new user evidence or a design change to reopen.
