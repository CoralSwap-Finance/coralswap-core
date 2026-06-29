# Security Policy

CoralSwap core contracts secure AMM, factory, pair, LP token, router, and
flash-loan receiver behavior. Please report suspected vulnerabilities
privately so maintainers can investigate and coordinate a fix before public
disclosure.

## Scope

Security reports are in scope when they affect assets, protocol integrity, or
availability in this repository, including:

- Soroban smart contracts under `contracts/`
- Shared contract interfaces and deployment configuration
- Build, test, and release files that could affect deployed contract behavior
- Documentation that could cause unsafe deployment or integration choices

Out of scope:

- Issues that require compromised private keys or user devices
- Social engineering, spam, or denial-of-service against public infrastructure
- Vulnerabilities in third-party services unless they directly affect this
  repository's contracts or deployment flow
- Reports without enough reproduction detail to assess impact

## Reporting a Vulnerability

Do not open a public GitHub issue for an unpatched vulnerability.

Send reports to the CoralSwap security team:

- Email: security@coralswap.finance
- If that address is unavailable, contact the maintainers through the GitHub
  organization and request a private security channel.

Include:

- Affected contract, file, function, or deployment step
- Reproduction steps or a proof of concept
- Expected impact and any affected assets
- Suggested mitigation, if known
- Your preferred contact for follow-up

## Response Targets

The team aims to acknowledge new reports within 3 business days.

Typical targets after acknowledgement:

- Triage and severity assessment: 7 business days
- Fix plan for confirmed high or critical issues: 14 business days
- Coordinated disclosure once a fix or mitigation is available

Timelines may vary with severity, exploitability, and deployment status.

## Bounty Notes

If a bug bounty program is active, eligibility, reward amount, and payout method
are determined by the bounty listing or campaign terms. A report is not eligible
when it is public before maintainers have had a reasonable chance to remediate
it, duplicates a known issue, or falls outside the scope above.

## Safe Harbor

Good-faith research that follows this policy, avoids privacy violations, avoids
service disruption, and does not access or move funds without authorization will
be treated as authorized security research by this project.
