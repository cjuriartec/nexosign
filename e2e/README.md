# Pruebas end-to-end (Playwright)

## Qué está automatizado hoy

| Spec | Qué valida | Entorno |
|------|------------|---------|
| `smoke.spec.ts` | UI estática contra build/`vite preview` | Solo frontend |
| `sign-route.spec.ts` | Ruta `/sign?intent=` y mensajes básicos | Solo frontend |
| `api-contract.spec.ts` | Contrato HTTP real (`/health`, batch intent, manifest…) | Requiere proceso HTTP NexoSign en **localhost:14500** |

Los tests que llaman a la API local comprueban `process.env.NEXOSIGN_E2E_API` (típicamente `http://127.0.0.1:14500`). Sin servidor Axum en ese puerto, esos casos se omiten o fallan según el propio spec.

## Qué sigue siendo manual o de alta fricción

- **App Tauri empaquetada / firma real**: PKCS#11, PIN, token físico y flujo completo **portal → intent → asistente → firma → descarga** no están cubiertos por Playwright en CI de forma fiable.
- **TTL de intents / tiempo simulado**: validar `GET …/intent/{id}/status` tras caducidad exige control del reloj o esperas largas; el contrato HTTP equivalente se cubre principalmente en tests **Rust** (`http_contract`, tests del router).
- **Páginas Colas, Certificados, Ajustes** y diálogo **origen no confiable**: útiles como smoke manual o futura automatización con Tauri driver; no son prerequisito del contrato API.

## Recomendación

Mantener **tests de contrato HTTP en Rust** como fuente principal para la API local; usar estos e2e para **smoke de UI** y, si se automatiza el binario, un único **happy path** documentado con variables de entorno claras.
