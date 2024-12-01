# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.x.x (latest) | Yes |
| Older releases | No |

Only the latest release receives security updates. Users should always run the most recent version.

---

## Reporting a Vulnerability

If you discover a security vulnerability in iVZA, please report it responsibly. Do NOT open a public GitHub issue for security vulnerabilities.

### How to Report

1. Send an email to the maintainers with the subject line: `[SECURITY] iVZA vulnerability report`.
2. Include the following information:
   - Description of the vulnerability.
   - Steps to reproduce the issue.
   - Affected components (core engine, on-chain program, SDK, CLI).
   - Potential impact assessment.
   - Any suggested fix, if available.

### What to Expect

- **Acknowledgment**: We will acknowledge receipt of your report within 48 hours.
- **Assessment**: We will assess the vulnerability and determine severity within 5 business days.
- **Fix timeline**: Critical vulnerabilities will be patched within 7 days. High-severity issues within 14 days. Medium and low severity within 30 days.
- **Disclosure**: We will coordinate with you on public disclosure timing. We follow a 90-day disclosure policy: if a fix is not released within 90 days, you are free to disclose publicly.

---

## Scope

The following components are in scope for security reports:

- **On-chain programs** (programs/ivza-engine): Account validation, instruction handling, state management, fund safety.
- **Core engine** (core/): Transaction graph construction, dependency analysis, execution logic.
- **TypeScript SDK** (sdk/): Key handling, transaction signing, RPC communication.
- **CLI** (cli/): Input validation, credential handling.

The following are out of scope:

- Vulnerabilities in Solana itself, Anchor, or other upstream dependencies (report these to the respective projects).
- Denial-of-service attacks against public RPC endpoints.
- Social engineering attacks.

---

## Security Practices

- All on-chain programs undergo account validation using Anchor's constraint system.
- The engine never stores or logs private keys.
- Transaction signing is performed client-side only.
- Dependencies are monitored via Dependabot for known vulnerabilities.
- CI runs `cargo clippy` and `cargo audit` to catch common issues.
