# NexoSign

Aplicación de escritorio **Tauri 2** + **SvelteKit** + **TypeScript**. API local HTTP embebida en **`127.0.0.1:14500`** (Axum) con CORS dinámico y eventos IPC `progreso` (fase 1).

## Prerrequisitos

- [Node.js](https://nodejs.org/) (LTS recomendado)
- [Rust](https://www.rust-lang.org/tools/install) y [prerrequisitos Tauri](https://v2.tauri.app/start/prerequisites/)

## Desarrollo

```bash
npm install
npm run tauri dev
```

El frontend Vite escucha en **`http://localhost:1420`**. La API Rust escucha en **`http://127.0.0.1:14500`**.

### Orígenes CORS (desarrollo)

Por defecto se permiten `localhost` / `127.0.0.1` en los puertos **1420** (Tauri+Vite) y **5173**. Para añadir más:

```bash
export NEXOSIGN_ALLOWED_ORIGINS="https://mi-app.example,http://localhost:3000"
npm run tauri dev
```

## Pruebas (matriz)

| Capa | Comando | Qué valida |
|------|---------|------------|
| **Dominio Rust** | `cargo test -p nexosign --lib domain` | Normalización de orígenes y política `AllowedOrigins` |
| **HTTP (Axum)** | `cargo test -p nexosign --lib adapters::http` | `/health`, `/api/v1/ping`, CORS preflight, rechazo de `Origin` no listado |
| **Contrato HTTP (crate)** | `cargo test -p nexosign --test http_contract` | Integración del router sin levantar proceso OS |
| **Cliente TS** | `npm run test` | Vitest: `fetchHealth` / `fetchPing` con `fetch` mockeado |
| **E2E UI** | `npx playwright install chromium` (una vez) · `npm run test:e2e` | Playwright + `vite preview`: título NexoSign y secciones visibles |
| **E2E API opcional** | Ver abajo (dos terminales) | `GET /health` real contra `:14500` |

**E2E API (`NEXOSIGN_E2E_API=1`):** la API solo existe mientras corre la app Tauri. Terminal A: `npm run tauri dev`. Terminal B: `NEXOSIGN_E2E_API=1 npm run test:e2e`. Si no hay nada en `:14500`, ese test se **omite** con mensaje (no cuenta como fallo).

Atajo recomendado tras instalar Rust:

```bash
npm run test              # Vitest
npm run test:e2e          # Playwright (smoke UI)
cargo test --manifest-path src-tauri/Cargo.toml   # Todo lo Rust
```

## IDE recomendado

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
