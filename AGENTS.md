# AGENTS.md — Guía para agentes y desarrolladores

Este documento define cómo debe evolucionar **NexoSign**: aplicación **Tauri** con núcleo **Rust**, API local y frontend **Svelte + TypeScript**. Es la fuente de verdad de arquitectura y disciplina de código de nivel senior.

---

## 1. Objetivos de calidad

- **Escalabilidad**: crecer por *módulos acotados* y *interfaces estables*, no por archivos monolíticos.
- **Testabilidad**: el dominio y los casos de uso deben poder probarse **sin hardware**, **sin red** y **sin UI**.
- **Seguridad**: superficie mínima en la API local; decisiones de confianza explícitas y persistidas.
- **Mantenibilidad**: SOLID y patrones solo donde reducen acoplamiento real; evitar ceremonia innecesaria.

---

## 2. Arquitectura hexagonal (Ports & Adapters)

### 2.1 Núcleo (sin frameworks)

| Capa | Responsabilidad | Depende de |
|------|-----------------|------------|
| **Dominio** | Entidades, invariantes, políticas puras (ej. “solo certificados de firma”) | Nada externo |
| **Casos de uso (application)** | Orquestación: “firmar lote”, “validar origen”, flujos | Solo dominio + **puertos** (traits/interfaces) |

Los casos de uso **no** importan Axum, Tauri, `cryptoki` ni SQLite directamente.

### 2.2 Puertos (traits en Rust / interfaces en TS)

Definir contratos explícitos:

- **`CertificateProvider`**: listar certificados de firma.
- **`Signer`**: firmar digest/CMS según contrato PAdES acordado.
- **`OriginPolicyStore`**: consultar y persistir orígenes permitidos.
- **`ProgressNotifier`**: notificar progreso (impl: eventos Tauri o no-op en tests).
- **`PdfSigningEngine`**: preparar PDF, hash, ensamblar firma (si se divide en pasos, mantener interfaces pequeñas).

### 2.3 Adaptadores

| Adaptador | Implementa | Ubicación típica |
|-----------|------------|------------------|
| HTTP (Axum) | Entrada: handlers delgados que llaman a un caso de uso | `src-tauri/src/adapters/http/` |
| Tauri commands/events | Misma regla: comandos = traducción DTO ↔ caso de uso | `src-tauri/src/adapters/tauri/` |
| PKCS#11 (`cryptoki`) | `CertificateProvider` / sesión token | `src-tauri/src/adapters/pkcs11/` |
| SQLite | `OriginPolicyStore` | `src-tauri/src/adapters/persistence/` |
| Cola batch | Un worker serial consumiendo la cola; usa `Signer` | `src-tauri/src/adapters/worker/` |

**Regla de oro**: si cambias de Axum a otro servidor o de SQLite a otro store, **el dominio y los casos de uso no deben cambiar**.

---

## 3. Principios SOLID (aplicados)

- **S**: Un archivo / tipo = una razón para cambiar. Handlers HTTP no contienen lógica de negocio.
- **O**: Extender comportamiento vía nuevos adaptadores o estrategias que cumplen el mismo puerto.
- **L**: Las implementaciones de puertos deben ser sustituibles (tests con doubles, otro HSM, etc.).
- **I**: Traits pequeños (`SignDigest`, `ListSigningCerts`) mejor que un “god trait”.
- **D**: Casos de uso dependen de **abstracciones** (puertos), no de `cryptoki::Session` concreto.

Frontend: vistas `.svelte` mayormente presentacionales; lógica de orquestación (`invoke`, `listen`) en **módulos `.ts`** (servicios, stores o `<script>` mínimo); no mezclar reglas de negocio en el markup.

---

## 4. Patrones de diseño recomendados

| Patrón | Uso en este proyecto |
|--------|----------------------|
| **Ports & Adapters** | Estructura principal (hexagonal). |
| **Application Service** | Un tipo por caso de uso (`SignBatchService`, `AuthorizeOriginService`). |
| **Strategy** | Variantes de firma o de motor PDF si hay varios perfiles PAdES. |
| **Factory** | Creación de sesión PKCS#11 o selección de slot según configuración. |
| **Observer / eventos** | Progreso y estado del lote → `ProgressNotifier` → Tauri `emit`. |
| **Queue + worker único** | Cola **secuencial** para PKCS#11 (hardware no paralelizable). |
| **Anti-corruption layer** | DTOs HTTP separados de entidades de dominio; mapeo explícito. |

Evitar **singletons globales ocultos**; preferir composición en el arranque (`setup`) e inyección vía `AppState` tipado.

---

## 5. Estructura de carpetas sugerida (`src-tauri`)

```
src-tauri/src/
  domain/           # entidades, errores de dominio, políticas puras
  application/    # casos de uso; solo usa domain + ports
  ports/          # traits (CertificateProvider, Signer, ...)
  adapters/
    http/
    tauri/
    pkcs11/
    persistence/
    worker/
  infrastructure/ # wiring: construir Axum, estado compartido, CORS dinámico
  lib.rs
```

El frontend es **Svelte + TypeScript**; puede espejar **features** por carpeta (`features/sign-batch/`, `features/settings/`) con UI en `.svelte` y lógica Tauri en `.ts` (servicios/stores).

---

## 6. Escalabilidad operativa

- **Rendimiento**: operaciones criptográficas en hilos adecuados (`spawn_blocking` si una librería bloquea); no bloquear el runtime de Tokio con PKCS#11 síncrono pesado sin aislar.
- **Timeouts**: configurables por operación (driver, firma, lectura PDF grande).
- **Memoria**: PDFs grandes → streaming o mmap donde aplique; no cargar todo el lote en RAM.
- **Observabilidad**: `tracing` con spans por trabajo de cola y correlación `job_id`.

---

## 7. Errores y tipos

- Errores de dominio **distintos** de errores de infraestructura; mapear a HTTP/Tauri en el borde.
- Evitar `String` como único error en propagación interna; usar `thiserror` o tipo enumerado coherente.

---

## 8. Tests

- **Dominio / casos de uso**: tests unitarios puros con mocks de puertos.
- **Adaptadores**: tests de integración con SoftHSM o PKCS#11 mock cuando sea posible.
- **HTTP**: contratos de API (origen CORS, códigos, forma del body).

---

## 9. Seguridad (recordatorio)

- CORS dinámico alineado con la misma lista persistida que autoriza orígenes.
- No loguear PIN ni datos sensibles de certificados.
- Deep links y callbacks validados antes de ejecutar acciones privilegiadas.

---

## 10. Relación con `.cursor/rules`

Las reglas en `.cursor/rules/*.mdc` **refuerzan** lo aquí descrito en formato corto para el agente. Ante conflicto, **este documento** prevalece hasta que se actualice explícitamente.
