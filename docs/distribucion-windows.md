# Distribución Windows (MSI / NSIS)

NexoSign usa el empaquetador de [Tauri 2](https://v2.tauri.app/distribute/windows-installer/). En [`src-tauri/tauri.conf.json`](../src-tauri/tauri.conf.json), `bundle.targets` incluye `all` (MSI vía **WiX**, NSIS, etc. según plataforma de build).

## Requisitos en la máquina de build (Windows)

- [Prerrequisitos Tauri](https://v2.tauri.app/start/prerequisites/) para Windows.
- **WiX Toolset v3** si quieres artefacto **`.msi`** (`cargo tauri build` lo selecciona cuando `targets` incluye `msi` / `all`).
- Opcional: característica **VBScript** de Windows si `light.exe` falla (Tauri documenta activarla en *Opcional features*).

El instalador **MSI solo se puede generar en Windows** (WiX no está disponible como toolchain cruzado típico desde macOS/Linux).

## Comando de build

Desde la raíz del repo:

```bash
npm install
npm run tauri build
```

Los artefactos suelen quedar bajo `src-tauri/target/release/bundle/` (`.msi`, `.exe`, etc.).

## Firma de código (opcional y recomendable para usuarios finales)

1. Obtener un certificado de firma de código compatible con **`signtool`** (p. ej. DigiCert, Sectigo, etc.).
2. Tras el build, firmar el ejecutable y/o el MSI según la práctica de tu CA (a menudo un `.pfx` en máquina segura o HSM).
3. No subir el `.pfx` ni contraseñas al repositorio; usar secretos de CI si automatizas.

Comando típico (ajusta huellas y rutas):

```powershell
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /sha1 THUMBPRINT "path\to\NexoSign.exe"
```

Consulta la documentación actual de Microsoft para **`signtool`** y sellado de tiempo.

## CI

Un job `windows-latest` que ejecute `npm ci`, `npm run tauri build` y archive el `.msi` como artefacto es el patrón habitual. Las credenciales de firma deben inyectarse como secretos del sistema de CI, no como ficheros en git.
