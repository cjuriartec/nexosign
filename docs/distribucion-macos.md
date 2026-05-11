# Distribución macOS (firma, notarización, staples)

Resumen operativo para publicar NexoSign fuera de la Mac de desarrollo. Detalle normativo: [Apple notarization](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution).

## 1. Certificado Developer ID Application

- Cuenta **Apple Developer Program**.
- En Xcode / Certificates crear **Developer ID Application** (no “Apple Development” para fuera de App Store).
- Instala el certificado y la cadena en **Keychain Access** de la máquina que firma.

## 2. Firma con `codesign`

Tras `npm run tauri build`, firma el `.app` y los binarios internos según la guía Tauri (bundle ya estructura `Contents/MacOS/…`). Ejemplo genérico:

```bash
codesign --deep --force --options runtime \
  --sign "Developer ID Application: TU NOMBRE (TEAM_ID)" \
  "target/release/bundle/macos/NexoSign.app"
```

Sustituye la cadena de firma por la que muestra `security find-identity -v -p codesigning`.

## 3. Notarización con `notarytool`

Usa **contraseña de app específica** o **perfil de API** según la práctica actual de Apple:

```bash
xcrun notarytool submit NexoSign.zip --apple-id "tu@correo.com" \
  --team-id TEAMID --password "@keychain:AC_PASSWORD" --wait
```

(O el archivo que envíes: `.dmg`, `.zip` del `.app`, según tu flujo.)

## 4. Staple

```bash
xcrun stapler staple "NexoSign.app"
```

Sin staple, Gatekeeper puede comportarse distinto offline.

## Entitlements y PKCS#11

La app carga bibliotecas PKCS#11 externas (`.dylib` del fabricante). Con **hardened runtime**, enlazar plugins puede requerir **entitlements** adicionales según el middleware:

- Algunos tokens necesitan **`com.apple.security.cs.disable-library-validation`** para cargar `.dylib` no firmados por Apple (valorar el riesgo; idealmente el proveedor distribuye bibliotecas firmadas).

Define entitlements en un `.plist` y pásalos a `codesign` con `--entitlements`. **Valida siempre con el driver PKCS#11 real** que usen tus usuarios.

Este repo no incluye un `entitlements.plist` único universal porque depende del proveedor del token.

## Secretos en CI

Para notarizar desde GitHub Actions u otro CI:

| Secreto típico | Uso |
|----------------|-----|
| Certificado exportado (`.p12`) + passphrase | Firma (preferible firma en runner efímero + borrar) |
| Apple ID + contraseña de app | `notarytool` |
| Team ID | `--team-id` |

No commitees credenciales; usa variables secretas del sistema de CI.

## Referencias

- [Tauri — macOS application bundle](https://v2.tauri.app/distribute/macos-application-bundle/)
- [Apple — Notarizing macOS software](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)
