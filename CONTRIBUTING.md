# Contribuir a NexoSign

Gracias por dedicar tiempo al proyecto. Esta guía resume cómo preparar el entorno, qué convenciones seguimos y cómo validar los cambios antes de abrir un pull request.

## Requisitos

- **Node.js** (LTS) y `npm`
- **Rust** estable y [prerrequisitos de Tauri 2](https://v2.tauri.app/start/prerequisites/)
- Opcional: **Playwright** (`npx playwright install chromium`) para E2E

## Arranque rápido

```bash
npm install
npm run tauri dev
```

- Frontend: `http://localhost:1420`
- API local: `http://127.0.0.1:14500`

## Arquitectura y límites

La fuente de verdad arquitectónica es **`AGENTS.md`** (hexagonal: dominio, casos de uso, puertos, adaptadores).

| Área | Regla breve |
|------|----------------|
| `src-tauri/src/domain/` | Sin Axum, Tauri, SQLite ni `cryptoki`. |
| `src-tauri/src/application/` | Orquestación vía traits en `ports/`. |
| `src-tauri/src/adapters/` | HTTP, Tauri, PKCS#11, persistencia, worker. |
| Frontend | Vistas `.svelte` mayormente presentacionales; orquestación en `.ts`. |

Si tu cambio cruza capas, enlaza en la descripción del PR cómo respeta esos límites.

## Estilo de código

- **Rust:** `cargo fmt` / `cargo clippy`; errores tipados (`thiserror` donde aplique); sin loguear PIN ni datos sensibles del token.
- **TypeScript / Svelte:** coherente con el código existente; `npm run check` debe pasar.
- **Commits:** mensajes claros en español o inglés; una idea principal por commit cuando sea posible.

## Qué ejecutar antes de un PR

```bash
npm run check        # Svelte + TypeScript
npm run test         # Vitest
cargo test --manifest-path src-tauri/Cargo.toml
```

Si tocas UI o rutas críticas:

```bash
npm run test:e2e
```

Para contratos HTTP contra API real (terminal aparte con `npm run tauri dev`):

```bash
NEXOSIGN_E2E_API=1 npm run test:e2e
```

## Pull requests

1. Describe **qué** cambia y **por qué** (contexto de usuario o bug).
2. Adjunta capturas si hay cambio visual relevante.
3. Si el cambio es grande, considera fragmentarlo; puedes consultar el flujo de trabajo habitual del equipo.

## Seguridad

Si encuentras una vulnerabilidad, **no** abras un issue público con el exploit completo: contacta al mantenedor por el canal que acordéis.

## Licencia

Al contribuir, aceptas que tu aporte se publique bajo la misma licencia del repositorio (**MIT**, salvo que se indique lo contrario).
