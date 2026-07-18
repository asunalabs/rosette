# Spike (T17): anti-enumeration mechanism for directory search

*Written recommendation + cost/complexity comparison, per T17's verify
criterion in `docs/plans/tasks-identity-directory-pivot.md`. No production
code — this unblocks T3's implementation, it doesn't do it.*

## Problem

T3 needs a phone-hash/username search endpoint that doesn't let an
authenticated caller enumerate the full registered-user set (the
compelled-disclosure-oracle concern from TODOS.md #1, and the reason T3 has
its own gate independent of T4's "must be authenticated" and T9's "no
query-content logging"). Rate-limiting alone was already rejected in the
task list — it slows enumeration, it doesn't prevent it, and a compelled
request doesn't care about rate limits.

## Options considered

| Option | How it works | Security | Complexity | Verdict |
|---|---|---|---|---|
| **Bloom filter, client-side** | Server ships a bloom filter of all registered phone-hashes; client tests membership locally. | **Worse than doing nothing.** Once fetched, the whole set can be tested offline, unlimited, unrateable, undetectable — this converts a rate-limitable online oracle into an unrateable offline one. | Low to build, but the security property is actively wrong for this threat model. | **Rejected.** |
| **Real PSI / OPRF** | Cryptographic protocol (oblivious PRF or similar) where server learns nothing about the query beyond "was there a match," client learns nothing beyond the match itself. | Strongest — this is the actual gold standard. | High. Signal's own writeup calls early PSI protocols for this exact problem "quite a disappointment" in practice — they shipped an SGX trusted-enclave design instead, and only later research (OPRF-based) closed the performance gap, with real engineering investment behind it. `[web search — verified]` | **Rejected for v1.** Right long-term target, wrong scope for a pre-launch solo project — this is a multi-week-to-multi-month cryptographic engineering effort, not a spike-sized decision. |
| **Trusted execution (SGX enclave)** | Server-side enclave computes the lookup without the host OS/operator learning the query. | Strong, and what Signal actually shipped first. | High — needs enclave-capable infra, remote attestation, and an ongoing SGX security posture (SGX itself has had its own side-channel CVEs over the years). | **Rejected for v1.** Infra dependency this project doesn't have and doesn't need yet. |
| **k-anonymity via truncated hash prefix** (HIBP-style) | Client sends a short prefix of `phone_hash` (not the full hash); server returns every registered entry whose hash shares that prefix — a same-shaped bucket of size *k*, match or not. Client does the exact match locally. | Moderate: server learns "which bucket," not "which entry." Bucket size *k* is the actual anonymity set — tune prefix length so *k* stays in a reasonable range (HIBP uses 20 bits / 5 hex chars, median bucket ~305, chosen as the response-size/anonymity sweet spot). `[web search — verified]` | Low. Prefix index + bucket lookup + rate limiting; no new infra, no unproven crypto. | **Recommended for v1.** |
| **Rate limiting alone** | — | Already rejected in the task list (T3's own note: "does not ship as v1"). | — | **Rejected**, no change from existing decision. |

## Recommendation: k-anonymity hash-prefix bucketing, HIBP-style

**Mechanism:** `phone_hash` (already Argon2id + pepper, from T2) gets
truncated to a fixed-length prefix for indexing. A search request sends
the prefix; the server returns the full set of `(phone_hash, user_id)`
pairs sharing that prefix — always the same *shape* of response (a bucket),
regardless of whether the caller's actual target is in it. The client does
the final exact-match comparison locally; the server never learns which
specific hash (or whether *any*) was the real target.

**Why this satisfies T3's own verify criterion, not just the spike's:**
T3 requires a CI timing-variance assertion — "response time distributions
statistically indistinguishable, not spot-checked." A bucket-shaped
response is naturally close to constant-time/constant-size: the server
does the same bucket lookup and returns the same shape whether the target
exists or not. This is a much easier property to hold under CI than trying
to make an exact-match single-hash lookup constant-time under real-world
DB/cache variance.

**Why this satisfies OQ9's data-minimization signoff (not a new decision,
just confirms it holds):** the server never logs or needs to log which
exact hash within a bucket was the target — consistent with T9's existing
"ephemeral counters, no query-content logging" design. There's genuinely
nothing more specific to hand over than "someone queried bucket N," which
is the whole point of the compelled-disclosure mitigation the external
legal review signed off on.

**Tuning knob — prefix length / target bucket size *k*:** Pick prefix
length so expected bucket size stays roughly constant as the user base
grows (HIBP's approach: fixed 20-bit prefix, letting bucket size grow with
corpus size rather than fixing bucket size and varying prefix length).
Given this directory starts at zero users, a fixed prefix length chosen
for an *anticipated* scale (not today's near-zero count) is the right
starting point — a bucket of 1-2 entries at low user counts still leaks
too much. Concretely: pick prefix length such that at expected v1 scale
(low thousands of users) buckets average in the tens, and revisit at T10
(schema/migration time) once real growth curves exist. This is a genuine
open tuning question, not resolved by this spike — flagging so T3's
implementation doesn't silently hardcode a number nobody chose on purpose.

**Explicit non-goal:** this is not "PSI, but worse" — it's a different,
lower-complexity point on the tradeoff curve, chosen because the
alternative (real PSI/OPRF) is disproportionate engineering for this
project's current stage. Revisit real PSI once user count, funding, or
threat model changes enough to justify it — same trigger point as the
scale threshold recorded in the private legal review for T3/OQ9.

## Unblocks

T3 can proceed to implementation using this design. T17 is complete —
this document is its deliverable.

---

Sources:
- [Have I Been Pwned: Pwned Passwords](https://haveibeenpwned.com/Passwords)
- [Understanding Have I Been Pwned's Use of SHA-1 and k-Anonymity — Troy Hunt](https://www.troyhunt.com/understanding-have-i-been-pwneds-use-of-sha-1-and-k-anonymity/)
- [Signal >> Technology preview: Private contact discovery for Signal](https://signal.org/blog/private-contact-discovery/)
- [Mobile Private Contact Discovery at Scale — Kales et al., USENIX Security 2019](https://www.usenix.org/system/files/sec19-kales.pdf)
