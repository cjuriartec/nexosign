# Actualizaciones automáticas (GitHub Releases)

NexoSign usa el [plugin Updater de Tauri](https://v2.tauri.app/plugin/updater/) con **GitHub Releases** como almacén estático. No hace falta un servidor propio.

## Cómo funciona

1. Cada release (`v*.*.*`) publica instaladores, archivos `.sig` y un `latest.json` unificado.
2. La app instalada consulta:
   `https://github.com/cjuriartec/nexosign/releases/latest/download/latest.json`
3. Si hay una versión más nueva, muestra un diálogo nativo y, si aceptas, descarga, verifica la firma e instala.

## Cuándo se comprueba

| Momento | Comportamiento |
|---------|----------------|
| Menú de bandeja → **Buscar actualizaciones** | Comprueba ya; si no hay nada nuevo, informa |
| **Ajustes** → Buscar actualizaciones | Igual que la bandeja |
| Cada **12 h** con el proceso activo | Comprueba en silencio; solo pregunta si hay update |

No se comprueba al arrancar el PC ni al abrir la ventana por primera vez (solo tras el primer intervalo de 12 h en segundo plano).

## Claves de firma del updater

La firma del updater **no** sustituye la firma de código de Windows/macOS (SmartScreen / Gatekeeper). Solo garantiza que el paquete de actualización lo publicó quien tiene la clave privada.

### Generar (una vez, en el proyecto)

Las claves viven en **`.secrets/`** en la raíz del repo. Esa carpeta está en `.gitignore` y **no se sube a Git**.

En la raíz del proyecto:

```bash
npm run updater:generate-keys -- "tu-contraseña-segura"
```

Usa solo letras y números en la contraseña (evita `>`, `?`, `|`, etc.).

El script crea:

| Archivo | Uso |
|---------|-----|
| `.secrets/nexosign.key` | Privada → secreto `TAURI_SIGNING_PRIVATE_KEY` |
| `.secrets/nexosign.key.pub` | Se aplica sola en `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` |

La contraseña del comando es la misma que va en `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Comprobar que Git ignora la carpeta:

```powershell
git check-ignore -v .secrets/nexosign.key
```

### Secretos de GitHub (Settings → Secrets and variables → Actions → Repository secrets)

| Secreto | Contenido |
|---------|-----------|
| `TAURI_SIGNING_PRIVATE_KEY` | Contenido completo del archivo `.key` (una sola línea base64) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | La contraseña que elegiste al generar la clave |

El workflow [`.github/workflows/release.yml`](../.github/workflows/release.yml) usa `includeUpdaterJson: true` y esas variables para firmar los artifacts en CI.

Si rotas las claves, actualiza el `pubkey` en `tauri.conf.json` y el secreto en GitHub; las versiones antiguas solo aceptarán updates firmados con la clave que conocen.

## Primera versión con updater

Los usuarios en builds **sin** updater (p. ej. v1.0.2) deben instalar **manualmente una vez** la primera versión que incluya esta función. A partir de ahí, las siguientes pueden llegar por el updater.

## Limitaciones

- **Repositorio público**: la descarga de `latest.json` y de los bundles es anónima. En repos privados haría falta autenticación adicional.
- **`tauri dev`**: las actualizaciones están deshabilitadas en builds de desarrollo (`debug_assertions`).
- **Instaladores sin firmar en CI**: SmartScreen/Gatekeeper pueden seguir avisando; ver [distribucion-windows.md](./distribucion-windows.md) y [distribucion-macos.md](./distribucion-macos.md).

## Publicar una release con updater

1. Bump de versión en `src-tauri/tauri.conf.json` y `src-tauri/Cargo.toml`.
2. Commit y tag: `git tag v1.0.x && git push origin v1.0.x`
3. Comprobar en la release de GitHub que existen:
   - Instaladores (`.exe`, `.dmg`, …)
   - Archivos `.sig`
   - `latest.json` con entradas para Windows y macOS (aarch64 + x86_64)

## Probar localmente

1. Instala un build release anterior.
2. Publica una release nueva con versión superior.
3. Bandeja → **Buscar actualizaciones** o Ajustes → **Buscar actualizaciones**.
