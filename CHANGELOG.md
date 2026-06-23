# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.3] - 2026-05-24

### Added

- **Signing wizard:** compose panel and clearer certificate visibility in the sign flow.
- **Certificates:** improved picker layout, empty states, and feedback in settings.

### Changed

- **Signing wizard:** more compact steps and action bar (less explanatory text, clearer flow).
- **Dependencies:** frontend security updates (`vitest`, `vite`, `svelte`, `cookie` override).

### Fixed

- **CORS:** only allowed origins receive `Access-Control-Allow-Origin` (no reflection for unknown origins).
- **Certificates:** tab layout regressions; PKCS#11 import on macOS; signing-cert visibility policy on non-Windows builds.
- **CI:** install `libpcsclite-dev` on Linux so Rust tests compile (`pcsc-sys`).

## [1.0.1] - 2026-05-24

### Changed

- **Windows (NSIS):** explicit `installMode: currentUser` so the setup installer targets `%LOCALAPPDATA%` without requiring administrator (app runtime never needed admin).
- **Docs:** clarify `*_x64-setup.exe` vs `.msi` install privileges in README and [docs/distribucion-windows.md](./docs/distribucion-windows.md).

## [1.0.0] - 2026-05-23

### Added

- **Tauri 2** desktop app (SvelteKit + Rust) for signing PDFs with **PAdES-BES**.
- **PKCS#11** integration (smart cards, Spanish DNIe): signing certificate listing, per-operation PIN, serial signing queue.
- **Windows**: signing certificates from the **Personal (MY)** store with RSA (CNG), deduplicated when the same thumbprint appears on the chip.
- **Local HTTP API** on `127.0.0.1:14500`: health, ping, batch via intent, status polling, signed PDF download; **OpenAPI** and Swagger UI.
- **Intent + deep link** flow (`nexosign://`) for web portals without exposing the PIN in the browser.
- Signing wizard: files or folder, stamp grid placement, certificate selection, confirmation, and progress results.
- **Visible signature** design (image + fields) and PNG generation for embedding in the PDF.
- **Allowed origins** policy (CORS) persisted; system tray and background operation with API active.
- Docs: Windows/macOS distribution, PKCS#11/MY certificates, tests (Vitest, Playwright, Rust HTTP contracts).

### Security

- Local loopback surface only; do not log PINs or token secrets.

[1.0.3]: https://github.com/cjuriartec/nexosign/releases/tag/v1.0.3
[1.0.1]: https://github.com/cjuriartec/nexosign/releases/tag/v1.0.1
[1.0.0]: https://github.com/cjuriartec/nexosign/releases/tag/v1.0.0
