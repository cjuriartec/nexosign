# Certificados PKCS#11, Windows y archivos .pfx

## Comportamiento actual de NexoSign

### PKCS#11 (todas las plataformas)

`list_signing_certificates` incluye certificados de **firma** (KeyUsage `nonRepudiation`) detectados vía **PKCS#11** (módulo `.dll` / `.so` / `.dylib`, sesión en slot, objetos `CKO_CERTIFICATE` en el token).

### Almacén Windows «Personal» (MY) — solo **Windows**

En **Windows**, la misma lista **añade** certificados del almacén **Current User / Personal (MY)** que cumplan:

- KeyUsage de firma (`nonRepudiation`),
- clave **RSA** con proveedor **CNG** (no se listan aquí certificados solo legacy CSP ni ECDSA),
- asociación con clave privada (`CERT_FIND_HAS_PRIVATE_KEY`).

El identificador en la UI es `winmy:` seguido de la huella **SHA-1** del certificado (hex). La firma PAdES-BES usa **NCrypt** con el mismo perfil CMS que PKCS#11.

### Deduplicación chip + MY

Si el mismo certificado (misma huella SHA-1 del DER) aparece en **PKCS#11** y en **MY**, `list_signing_certificates` devuelve **solo la entrada del lector (chip)**. Así la UI no duplica el mismo DNIe.

### Política de visibilidad (MY vs chip)

Tras fusionar PKCS#11 + MY y deduplicar, se aplica `apply_signing_cert_visibility_policy` en el dominio (`domain/signing_cert.rs`):

| Tipo en MY (`win_my_key_binding`) | ¿Hay certificado PKCS#11 en la lista? | ¿Se muestra? |
|-----------------------------------|----------------------------------------|--------------|
| `smart_card` o `unknown` | No | **No** (evita el DNIe “fantasma” solo en Windows) |
| `smart_card` o `unknown` | Sí | Oculto por dedupe; solo **Lector (chip)** |
| `software` | No | **Sí** (firma con clave en el PC) |
| `software` | Sí | Ambos si son certificados distintos |

La clasificación del binding usa el proveedor de clave en MY (`smart card`, `minidriver`, `software key storage`, etc.).

**UX:** si el lector detecta tarjeta pero la lista queda vacía (p. ej. el chip exige PIN para enumerar), la UI ofrece **Probar con PIN** en Certificados y en el paso 3 del asistente Firmar.

### PIN en la interfaz (`pin_ui`)

Para no mostrar un PIN «opcional» confuso, cada certificado de MY lleva un campo **`pin_ui`**:

| Valor | UI |
|--------|-----|
| `required_in_app` | Campo PIN obligatorio en NexoSign (p. ej. heurística smart card). |
| `hidden_use_os_crypto` | Sin campo PIN en NexoSign; uso típico de claves software en MY. |
| `os_may_prompt` | Sin PIN en la app; aviso de que **Windows o el dispositivo** pueden pedir confirmación al firmar. |

La clasificación usa el nombre del proveedor (`CERT_KEY_PROV_INFO`) y un intento silencioso de abrir la clave CNG.

### Qué sigue sin estar cubierto

- **macOS / Linux:** no hay integración con llavero del SO; solo PKCS#11.
- **`.pfx` solo en disco** sin instalar en MY ni middleware PKCS#11.
- **ECDSA** en MY (fase posterior si se requiere).
- **Claves solo CSP legacy** en MY (sin CNG): la firma devolverá error claro.

## Extensiones futuras (diseño alineado con AGENTS.md)

Para **.pfx en archivo** u otros orígenes, seguiría teniendo sentido un **puerto** tipo `CertificateProvider` / `Signer` y adaptadores adicionales.

| Enfoque | Notas |
|--------|--------|
| **B — Archivo .pfx + contraseña** | Multiplataforma vía Rust; política de PIN y memoria. |
| **C — Middleware PKCS#11** | Sin código en NexoSign si el proveedor expone `.dll`. |

Los casos de uso PAdES deberían seguir usando abstracciones (`PdfPadesSigner`, firmadores CMS) para no acoplar el dominio a Windows.
