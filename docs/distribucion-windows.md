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

## ¿Hace falta ser administrador?

**No para usar NexoSign.** La app (PKCS#11, PDF, API en `127.0.0.1`, SQLite, bandeja) funciona con privilegios de usuario normal.

Lo que puede pedir UAC es el **instalador**:

| Artefacto | Comportamiento típico |
| --------- | --------------------- |
| **`NexoSign_*_x64-setup.exe` (NSIS)** | Modo **`currentUser`** (configurado en `tauri.conf.json`): instala en `%LOCALAPPDATA%` **sin** exigir administrador en el asistente. |
| **`.msi` (WiX)** | Instalación clásica en `Program Files` → **sí suele pedir administrador**. Pensado para despliegue corporativo. |

### Modos NSIS (`bundle.windows.nsis.installMode`)

- **`currentUser`** (recomendado para usuarios finales): sin elevación en el instalador; solo para el usuario actual.
- **`perMachine`**: para todos los usuarios en `Program Files`; requiere administrador.
- **`both`**: el usuario elige en el asistente, pero Tauri documenta que el instalador **sigue ejecutándose con privilegios elevados** aunque elija “solo para mí”. No evita el aviso de UAC.

Si aun así aparece UAC con el `.exe`:

1. No ejecutes el instalador con “Ejecutar como administrador”.
2. Puede ser la instalación de **WebView2** (el runtime de Tauri): si el instalador de NexoSign **no** está elevado, Microsoft instala WebView2 por usuario; si está elevado, lo instala para todo el equipo.
3. En Windows 11, WebView2 suele estar ya instalado y ese paso se omite.

Para el próximo release, el cambio en `tauri.conf.json` aplica al volver a generar el instalador NSIS.

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
