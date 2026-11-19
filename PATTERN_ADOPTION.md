# PATTERN_ADOPTION

Relationship declaration to the WyattAu pattern canon
(pattern-standards): format per references/adoption-format.md in that
canon, gated by its adoption-check.py script.

Scope: web contact discovery over hostile HTML. No uploads, no
multi-tenant storage, no distributed state; the matching baseline
patterns are omitted per rule 6 of the format.

| Pattern | Status | Evidence |
|---|---|---|
| T1 | partial | src/parser.rs:50 (hostile HTML parses once into typed results: SearchResult and ContactInfo at src/parser.rs:224); email checks stay boolean validate at src/validator.rs:68 |
| P51 | adopted | src/http.rs:43 (3 consecutive failures trip a 30 minute cooldown at src/http.rs:45; success resets the counter at src/http.rs:93) |
| P70 | partial | src/parser.rs:50 (href-scoped extraction ignores unknown engine markup; Bing redirect unwrap at src/parser.rs:97) |
| P72 | partial | src/error.rs:4 (single MinerError envelope across http, parse, and store stages; string payloads, no structured codes) |

No refused rows: no pattern was consciously rejected with a recorded
decision. No planned rows: no dated tracking reference exists yet
(property tests and structure-aware fuzzing are absent from the code,
so P64 and P67 stay unclaimed rather than wished in).
