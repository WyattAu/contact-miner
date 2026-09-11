# Requirements — contact-miner

Numbered, testable requirements. Every requirement maps to at least one named
test or doc-comment contract; security-relevant items cite threat-model rows.

Scope: Contact extraction (`contact-miner`) — parses emails/phones/links from documents with verification heuristics

## Functional

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-CM-001 | Parser extracts well-formed emails/phones from text with position offsets | MUST |
| REQ-CM-002 | Verifier labels candidates with confidence; obfuscated forms (AT/domains) handled per documented rules | MUST |
| REQ-CM-003 | HTTP fetch/store/validate stages return typed errors, never panic on hostile input | MUST |

## Security

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-CM-100 | Parser bounds all captures (no unbounded allocation from hostile documents) | MUST |
| REQ-CM-101 | Regex is linear-time (no catastrophic backtracking) — `regex` crate | MUST |

## Observability & API hygiene

| ID | Requirement | Priority |
|----|-------------|----------|
| REQ-CM-900 | All fallible public APIs return typed errors; production `unwrap`/`expect` is denied or explicitly justified with an invariant comment | MUST |
| REQ-CM-901 | Public items carry doc comments with runnable examples where practical | SHOULD |
