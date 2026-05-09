# NexoSign

Aplicación de escritorio **Tauri 2** + **SvelteKit** + **TypeScript**. API local HTTP embebida en **`127.0.0.1:14500`** (Axum) con CORS dinámico y eventos IPC `progreso`. La **fase 2** añade descubrimiento PKCS#11, listado de certificados de firma y sesión con PIN + timeout de inactividad vía comandos Tauri.

### PKCS#11 / DNIe (fase 2)

| Variable | Descripción |
|----------|-------------|
| `NEXOSIGN_PKCS11_MODULE` | Ruta absoluta al `.dll` / `.so` / `.dylib` PKCS#11 (prioridad sobre rutas por defecto). |
| `NEXOSIGN_PKCS11_SLOT` | Índice del slot con token (`0` por defecto). |
| `NEXOSIGN_TOKEN_IDLE_SECS` | Segundos de inactividad antes de `logout` automático del token (por defecto `900`). |

En macOS / Windows el sistema puede **autenticar** el DNIe vía Apple / CryptoAPI sin usar el mismo stack que **PKCS#11**. Si el DNI “funciona” en el navegador pero NexoSign muestra **0 slots** o el lector sin `token_present`, suele ser el **módulo equivocado**: prueba el PKCS#11 del **middleware oficial del DNIe** (FNMT/CCN) y apunta `NEXOSIGN_PKCS11_MODULE` a su biblioteca. OpenSC a veces no ve la tarjeta aunque el lector USB sí esté enlazado.

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
