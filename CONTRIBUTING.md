# Contributing to NexoSign

Thank you for your interest in NexoSign. This guide explains how to set up your environment, follow project conventions, and validate changes before opening a pull request.

## Table of contents

- [Welcome](#welcome)
- [Before you start](#before-you-start)
- [How to contribute](#how-to-contribute)
- [Code standards](#code-standards)
- [Required checks](#required-checks)
- [Pull request checklist](#pull-request-checklist)
- [Bugs and security](#bugs-and-security)
- [Releases (maintainers)](#releases-maintainers)
- [License](#license)

## Welcome

**NexoSign** is a desktop app for signing PDFs with a hardware or Windows certificate (PKCS#11, PAdES), plus a local HTTP API and deep links for web integration.

Please read the [Code of Conduct](./CODE_OF_CONDUCT.md) and the [README](./README.md) for product context.

## Before you start

### Requirements

- **Node.js** (LTS) and `npm`
- **Rust** (stable) and [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/)
- Optional: **Playwright** (`npx playwright install chromium`) for E2E tests

### Quick start

```bash
npm install
npm run tauri dev
```

| Service   | URL                        |
| --------- | -------------------------- |
| Frontend  | `http://localhost:1420`    |
| Local API | `http://127.0.0.1:14500`   |

### Further reading

- **[AGENTS.md](./AGENTS.md)** — hexagonal architecture (domain, use cases, ports, adapters)
- **[docs/](./docs/)** — distribution, PKCS#11 / Windows MY, certificates

## How to contribute

1. **Fork** the repository on GitHub.
2. Create a branch from **`main`**, e.g. `feature/short-description` or `fix/issue-topic`.
3. Make focused changes (one topic per pull request when possible).
4. Run the [required checks](#required-checks).
5. Open a **pull request** against `main` with a clear description (English or Spanish is fine).

## Code standards

### Architecture

| Area                         | Rule                                                                 |
| ---------------------------- | -------------------------------------------------------------------- |
| `src-tauri/src/domain/`      | No Axum, Tauri, SQLite, or `cryptoki`.                             |
| `src-tauri/src/application/` | Orchestration via traits in `ports/` only.                           |
| `src-tauri/src/adapters/`    | HTTP, Tauri, PKCS#11, persistence, worker.                           |
| Frontend                     | `.svelte` views are mostly presentational; orchestration in `.ts`.   |

If your change crosses layers, explain in the PR how it respects these boundaries.

### Style

- **Rust:** `cargo fmt`; run `cargo clippy` when practical; use typed errors (`thiserror` where appropriate); **never** log PINs or sensitive token data.
- **TypeScript / Svelte:** match existing patterns; `npm run check` must pass.

### PKCS#11: PIN in the UI vs the token

The UI asks for the PIN **once per batch** before signing. The worker signs PDFs **serially** on a single PKCS#11 queue (do not parallelize chip operations).

Many tokens (e.g. DNIe) set **CKA_ALWAYS_AUTHENTICATE**, so the middleware may require **context-specific re-login** before each `C_Sign` even when the user entered the PIN only once. “Single PIN in UX” does not always mean a single PKCS#11 login for the whole batch. See `rsa_sha256_pkcs1_sign` in [`src-tauri/src/adapters/pkcs11/token.rs`](src-tauri/src/adapters/pkcs11/token.rs).

## Required checks

Run these before opening a PR:

```bash
npm run check
npm run test
cargo test --manifest-path src-tauri/Cargo.toml
```

| Layer            | Command                                              |
| ---------------- | ---------------------------------------------------- |
| Svelte / TS      | `npm run check`, `npm run test`                      |
| Rust (all)       | `cargo test --manifest-path src-tauri/Cargo.toml`    |
| Domain only      | `cargo test -p nexosign --lib domain`                |
| HTTP adapters    | `cargo test -p nexosign --lib adapters::http`        |
| HTTP contract    | `cargo test -p nexosign --test http_contract`        |

If you change UI or critical flows:

```bash
npm run test:e2e
```

E2E against the live API (separate terminal with `npm run tauri dev`):

```bash
NEXOSIGN_E2E_API=1 npm run test:e2e
```

E2E tests that need `:14500` are **skipped** when the server is not running.

### Local API troubleshooting

- **Only one NexoSign instance** should run; a second launch focuses the first window.
- If **`14500` is in use** by another app, the HTTP listener fails at startup (no silent fallback). Signing from the desktop UI still works via IPC; integrators need a free port or `NEXOSIGN_LOCAL_API_PORT`.
- In the app: **Settings → Servicio local** shows bind errors; **`get_local_api_status`** (Tauri) exposes the same state.

## Pull request checklist

- [ ] Description explains **what** changed and **why**
- [ ] Screenshots or short recording for visible UI changes
- [ ] `npm run check`, `npm run test`, and `cargo test` pass
- [ ] [CHANGELOG.md](./CHANGELOG.md) updated for user-facing changes (if applicable)
- [ ] No secrets, PINs, `.pfx`, or private keys committed
- [ ] Large changes split into smaller PRs when possible

## Bugs and security

- **Bugs and features:** use [GitHub Issues](https://github.com/cjuriartec/nexosign/issues) with the appropriate template.
- **Security vulnerabilities:** follow [SECURITY.md](./SECURITY.md). Do **not** file public issues with exploit details.

## Releases (maintainers)

- Versioning follows **Semantic Versioning** (e.g. `1.0.0`, `1.0.1`).
- User-facing changes are recorded in [CHANGELOG.md](./CHANGELOG.md).
- Pushing a tag matching `v*.*.*` triggers the [release workflow](.github/workflows/release.yml) to build Tauri bundles (Windows / macOS). Installers are **not** code-signed in CI for v1; see [docs/distribucion-windows.md](./docs/distribucion-windows.md).

## License

By contributing, you agree that your contributions are licensed under the [MIT License](./LICENSE), the same as the project.
