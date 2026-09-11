# Threat Model — contact-miner

Reference: STRIDE. Scope: the crate's public API surface. Trust boundary:
(1) bytes/inputs entering public constructors and parsers, (2) concurrent
callers sharing interior state. contact-miner is an in-process library — it opens
no sockets and inherits the embedding process's trust domain.

Purpose: Contact extraction (`contact-miner`) — parses emails/phones/links from documents with verification heuristics

## Assets

| ID | Asset | Exposed via |
|----|-------|-------------|
| A1 | bounded resource use on hostile input | hostile input, concurrent callers |
| A2 | precision of extracted contacts | hostile input, concurrent callers |

## STRIDE Analysis

| # | Threat | Category | Surface | Mitigation | Residual risk |
|---|--------|----------|---------|------------|---------------|
| T1 | DoS via pathological documents (size/backtracking) | DoS | `parser` | size caps + `regex` crate linear-time guarantees | documented |
| T2 | Junk classified as valid contact | Spoofing | `verifier` | confidence scoring with documented thresholds; tests pin known-good/bad cases | documented |
| T3 | SSRF via mined links | Info disclosure | `HTTP stage` | fetch stage is caller-driven; URL allowlisting is the embedding application's duty (documented residual risk) | documented |

## Repudiation

The crate keeps no audit trail; attribution of calls to callers is out of
scope for an in-process library.

## Out of Scope

- Network transport security (the crate never opens sockets).
- Storage-host compromise: an attacker who controls the host can bypass all
  in-process mitigations.
- Denial of service via resource exhaustion of the host process beyond the
  bounds enforced above.

Reviewed: 2026-09-11
