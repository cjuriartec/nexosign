<div align="center">

<div style="display:flex; flex-wrap:wrap; gap:8px; justify-content:center; align-items:center;">
<a href="https://tauri.app/"><img src="https://img.shields.io/badge/Tauri-2-24FFC8?style=for-the-badge&logo=tauri&logoColor=000" alt="Tauri 2"/></a>
<a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-202124?style=for-the-badge&logo=rust&logoColor=white" alt="Rust"/></a>
<a href="https://kit.svelte.dev/"><img src="https://img.shields.io/badge/SvelteKit-FF3E00?style=for-the-badge&logo=svelte&logoColor=white" alt="SvelteKit"/></a>
<a href="https://www.typescriptlang.org/"><img src="https://img.shields.io/badge/TypeScript-3178C6?style=for-the-badge&logo=typescript&logoColor=white" alt="TypeScript"/></a>
<a href="./package.json"><img src="https://img.shields.io/badge/licencia-MIT-6366f1?style=for-the-badge" alt="Licencia MIT"/></a>
</div>

<br/>

<img src="docs/assets/readme-hero.svg" alt="NexoSign — firma PDF en escritorio con certificado electrónico" width="92%"/>

<br/>

<div align="center" style="display:flex; flex-wrap:wrap; gap:12px 18px; justify-content:center; align-items:center; margin:12px 0;">
<strong>🔐 Local</strong>
<strong>📄 PAdES</strong>
<strong>🔌 PKCS#11</strong>
<strong>🌐 API loopback</strong>
<strong>🔗 Deep links</strong>
</div>

<div align="center" style="display:flex; flex-wrap:wrap; gap:10px 20px; justify-content:center; align-items:center;">
<a href="#-api-local--referencia-rápida">Ficha técnica</a>
<span aria-hidden="true">·</span>
<a href="#-integración-externa--portal-y-escritorio">Integración</a>
<span aria-hidden="true">·</span>
<a href="./CONTRIBUTING.md">Contribuir</a>
</div>

</div>

---

## ✨ Por qué NexoSign

| | |
|:---|:---|
| 🔒 **Privacidad por diseño** | La firma y el PIN ocurren **en el equipo del usuario**. La API solo escucha en **loopback** — no es un SaaS que centralice tus PDF ni tus claves. |
| 🖲️ **Hardware real** | PKCS#11: mismo modelo que **DNIe**, tarjetas y HSM. **Una cola, un firmador**: el paralelismo no rompe lo que el chip no permite. |
| 🧩 **Tu web, el escritorio** | Desde el navegador puedes registrar una **intención** (`POST …/intent`), recibir un **`deep_link`** y abrir la app con **`nexosign://`** para que el usuario **complete el asistente** (certificado, PIN, casilla). |
| ⚙️ **Automatización local** | La **app de escritorio** y herramientas en la misma máquina pueden seguir rutas HTTP internas no descritas en OpenAPI; integradores remotos usan **intent → estado → descargas**. |

---

## 🎯 Experiencia en la app

1. **Origen** — PDF sueltos o **carpeta completa** (todos los `.pdf`, también en subcarpetas). Carpeta → salida en **`NombreCarpeta_firmados`** junto a la carpeta elegida.
2. **Certificado** — Eliges entre certificados de **firma** detectados vía PKCS#11.
3. **PIN** — Solo para desbloquear el token en esa operación; **sin** sesión PKCS#11 prolongada por tiempo.
4. **Ubicación y confirmar** — Rejilla en primera página, cola local y seguimiento del lote.

📁 **Salida:** `{nombre}_firmado.pdf` junto al original o dentro de `…_firmados` si firmaste por carpeta.

---

## 🛰️ API local — referencia rápida

La API está en **`http://127.0.0.1:14500`** **solo con la aplicación en ejecución** (`npm run tauri dev` o binario instalado).

| Requisito | Detalle |
|-----------|---------|
| 🌍 **Origen** | Los **`POST` y `GET`** de batch en navegador necesitan cabecera **`Origin`** permitida por CORS (p. ej. `http://localhost:1420`). Incluye sondeo de estado del intent y descargas. |
| 💻 **`curl`** | Añade `-H "Origin: http://localhost:1420"` como en los ejemplos. |
| 📘 **OpenAPI** | Con la app en marcha: **`GET /openapi.json`** (solo endpoints pensados para integración externa: intent, estado, descargas). **`GET /docs`** abre **Swagger UI**. Importación en [Scalar](https://scalar.com), Postman, etc. |
| 📂 **Multipart vs firma** | **`multipart/form-data`** solo en **`POST …/batch/sign/intent`** (subir PDF desde el navegador). La firma la ejecuta la **app** tras el deep link; ese paso **no** está en **`openapi.json`** (contrato para integradores web). |

| Endpoint | Rol |
|----------|-----|
| **`GET /api/v1/batch/jobs/{job_id}/signed-files`** | Cuando el lote **ha terminado**: JSON con `files[]` (`index`, `filename`, `href`) para descargar cada PDF firmado **desde el navegador** (cabecera **Origin** igual que en los POST). |
| **`GET /api/v1/batch/jobs/{job_id}/files/{i}`** | Respuesta **`application/pdf`** del firmado *i*-ésimo (mismo orden que `inputs` / solo los firmados con éxito). |
| **`GET /api/v1/batch/sign/intent/{request_id}/status`** | **Sondeo** tras el intent: `phase` = `awaiting_confirmation` \| `processing` \| `completed`, `job_id`, `manifest_href`, `signed_file_count`. Sin servidor propio: tu página hace polling a `127.0.0.1:14500` con el mismo **Origin**. |
| **`POST /api/v1/batch/sign/intent`** | **No firma aún.** **`application/json`** (`inputs`: rutas absolutas) **o** **`multipart/form-data`** con el campo repetible **`files`** (un PDF por parte). Los PDF subidos van a staging temporal; responde **`request_id`** + **`deep_link`**. TTL ~30 min. |
| **`GET /health`** | Estado del servicio (sin `Origin`). |
| **`POST /api/v1/ping`** | Eco para pruebas. |
| **`NEXOSIGN_BATCH_OUTPUT_DIR`** | Variable de entorno: fuerza carpeta de salida global `{stem}_firmado.pdf`. |

---

## 🔗 Integración externa — portal y escritorio

Cuando el usuario **debe** elegir certificado y PIN **en la app** (no en un POST invisible desde tu servidor).

```mermaid
flowchart LR
  subgraph Web["Tu integración"]
    A[Página o agente local]
  end
  subgraph NexoSign["Equipo del usuario"]
    B["API 127.0.0.1:14500"]
    C["nexosign:// deep link"]
    D["App NexoSign — asistente"]
  end
  A -->|"POST /batch/sign/intent"| B
  B -->|"request_id + deep_link"| A
  A -->|"Abrir enlace"| C
  C --> D
  D -->|"Confirma y encola (uso interno)"| B
```

**Pasos**

1. **JSON:** los PDF ya están **en disco** (rutas absolutas) en ese PC.
2. **Multipart:** el navegador envía los PDF en el campo repetible **`files`** (hasta 20 PDF, 50 MiB c/u, techo de suma 20×50 MiB); la API materializa temporales y el asistente los trata como entradas del lote.
3. Tu cliente llama **`POST /api/v1/batch/sign/intent`** (JSON o multipart).
4. Recibes **`request_id`** y **`deep_link`** — úsalos en un botón del tipo **«Abrir en NexoSign»**.
5. El sistema operativo abre la app registrada para **`nexosign://`** (en desarrollo a veces conviene lanzar la app manualmente).
6. El usuario completa el asistente; al confirmar, **NexoSign** encola la firma **localmente** y relaciona el **`request_id`** del intent con el trabajo (`job_id`) para el sondeo y las descargas; al terminar el lote se **borra** el staging.

### Portal sin backend propio (solo HTML/JS en tu dominio)

No hace falta un callback HTTP a tu servidor. Con el **`request_id`** del intent:

1. Abres el **`deep_link`** (o equivalente) para lanzar NexoSign.
2. Desde la misma pestaña (origen ya autorizado en Ajustes), **sondea** cada pocos segundos:  
   `GET http://127.0.0.1:14500/api/v1/batch/sign/intent/{request_id}/status`  
   con cabecera **`Origin`** (la de tu portal).
3. Cuando **`phase`** sea **`completed`**, usa **`manifest_href`** (o el **`job_id`**) para  
   `GET …/batch/jobs/{job_id}/signed-files` y luego cada **`GET …/files/{i}`** y guardar los blobs en el cliente.

Mientras tanto verás **`awaiting_confirmation`** (intención abierta) o **`processing`** (trabajo encolado, firma en curso).

> **Nota:** si no añades un backend intermedio, el sondeo es siempre contra **loopback** desde el navegador del usuario (misma máquina que NexoSign).

> **Producto (histórico):** antes el portal no tenía forma de saber el `job_id`; el endpoint de **status** cierra ese flujo.

> **`GET /openapi.json`** solo incluye intent, estado del intent y descargas del lote (contrato para integradores). Rutas adicionales que usa la app en la misma máquina **no** aparecen ahí.

<details>
<summary><strong>📋 Ejemplo — intención subiendo PDF (multipart)</strong></summary>

```bash
curl -sS -X POST "http://127.0.0.1:14500/api/v1/batch/sign/intent" \
  -H "Origin: http://localhost:1420" \
  -F "files=@/Users/tu/usuario/documentos/doc.pdf;type=application/pdf"
```

</details>

<details>
<summary><strong>📋 Ejemplo — intención con rutas locales (JSON)</strong></summary>

```bash
curl -sS -X POST "http://127.0.0.1:14500/api/v1/batch/sign/intent" \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:1420" \
  -d "{\"inputs\": [\"/Users/tu/usuario/documentos/doc.pdf\"]}"
```

</details>

```json
{
  "request_id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
  "deep_link": "nexosign://sign?intent=f47ac10b-58cc-4372-a567-0e02b2c3d479"
}
```


### Fuera del OpenAPI público

La app de escritorio y otros procesos **locales** pueden usar rutas HTTP adicionales en loopback; **no** forman parte de **`openapi.json`**. Para una integración desde tu dominio, ceñirse a **intent → sondeo de estado → descargas**.

---

## 🧱 Capacidades técnicas

| | |
|:---|:---|
| 📜 **PAdES-BES** | CMS detached + RSA en token. |
| 🛡️ **CORS** | Lista de orígenes alineada con la política en app / SQLite. |
| 📊 **`progreso`** | Eventos IPC por documento para barras y logs. |
| 🔑 **PKCS#11** | Descubrimiento de módulos, certificados de firma, sesión acotada. |
| 🔐 **PIN** | En batch por loopback o comandos `pkcs11_login` / `pkcs11_logout` según flujo. |

---

## 💳 PKCS#11 / DNIe

| Variable | Uso |
|----------|-----|
| `NEXOSIGN_PKCS11_MODULE` | Ruta absoluta al `.dll` / `.so` / `.dylib` (prioridad sobre rutas por defecto). |
| `NEXOSIGN_PKCS11_SLOT` | Índice del slot (`0` por defecto). |

Si el DNIe funciona en el navegador del sistema pero NexoSign muestra **0 slots**, suele ser **middleware PKCS#11 distinto**: prueba el del **proveedor oficial del DNIe** (FNMT/CCN) con `NEXOSIGN_PKCS11_MODULE`. OpenSC a veces no expone la tarjeta aunque el USB sí esté reconocido.

---

## 📦 Prerrequisitos

- [Node.js](https://nodejs.org/) (LTS)
- [Rust](https://www.rust-lang.org/tools/install) y [prerrequisitos Tauri](https://v2.tauri.app/start/prerequisites/)

## 🚀 Desarrollo

```bash
npm install
npm run tauri dev
```

| Servicio | URL |
|----------|-----|
| Frontend | **`http://localhost:1420`** |
| API | **`http://127.0.0.1:14500`** |

### Orígenes extra (CORS)

```bash
export NEXOSIGN_ALLOWED_ORIGINS="https://mi-app.example,http://localhost:3000"
npm run tauri dev
```

Por defecto: `localhost` / `127.0.0.1` en puertos **1420** (Tauri+Vite) y **5173**.

---

## ✅ Pruebas

| Capa | Comando | Valida |
|------|---------|--------|
| Dominio Rust | `cargo test -p nexosign --lib domain` | Política de orígenes |
| HTTP | `cargo test -p nexosign --lib adapters::http` | Batch, intent, CORS |
| Contrato | `cargo test -p nexosign --test http_contract` | Router sin proceso OS |
| Cliente TS | `npm run test` | Vitest |
| E2E UI | `npm run test:e2e` | Playwright |
| E2E API | Terminal A: `npm run tauri dev` · B: `NEXOSIGN_E2E_API=1 npm run test:e2e` | Contrato contra API real |

Sin servidor en `:14500`, los E2E que llaman a red **se omiten** (no fallan).

Primera vez: `npx playwright install chromium`.

```bash
npm run test
npm run test:e2e
cargo test --manifest-path src-tauri/Cargo.toml
```

---

## 🤝 Contribuir

Las convenciones de código y el flujo de PR están en **[`CONTRIBUTING.md`](./CONTRIBUTING.md)**. La arquitectura detallada vive en **`AGENTS.md`**.

---

## 🛠️ IDE recomendado

[VS Code](https://code.visualstudio.com/) · [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) · [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) · [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
