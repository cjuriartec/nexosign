# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 1.0.x   | :white_check_mark: |
| < 1.0   | :x:                |

## Reporting a Vulnerability

**Do not open a public issue** with steps to exploit security flaws (PIN, PKCS#11 tokens, local API, deep links, etc.).

1. Use **[GitHub Security Advisories](https://github.com/cjuriartec/nexosign/security/advisories/new)** (private report) on the `cjuriartec/nexosign` repository.
2. If you cannot use Advisories, contact the maintainers privately (no exploit details in public).

Please include when possible:

- NexoSign version and operating system.
- Estimated impact (confidentiality, integrity, availability).
- Steps to reproduce **without** real certificate data or PINs.

We aim to acknowledge reports within a few business days and to ship a fix or mitigation before public disclosure, depending on severity.

## Out of Scope (typical)

- Physical access to the user’s PC while the PIN has already been entered.
- Deliberately insecure configuration (e.g. CORS allowing untrusted origins).
- Vulnerabilities in third-party PKCS#11 middleware (report to the driver vendor).
