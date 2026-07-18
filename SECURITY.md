# Security policy

Rosette is pre-audit, pre-release software. The cryptography is real (MLS
via OpenMLS), but the implementation has not had an external audit — the
audit gates public beta. Don't stake your safety on it yet.

## Reporting a vulnerability

Use GitHub's **private vulnerability reporting** on this repository
(Security tab → "Report a vulnerability"). Reports reach the maintainers
privately. Please don't open public issues for exploitable bugs. There is
no bug bounty yet.

## Known-open security work

Tracked in the open, on purpose: see [TODOS.md](TODOS.md) (e.g. TODO 12,
group-inbox roster validation) and
[docs/plans/tasks-identity-directory-pivot.md](docs/plans/tasks-identity-directory-pivot.md).
Pre-launch, there are no production deployments to exploit — the honest
list is cheaper than the illusion of a clean one.
