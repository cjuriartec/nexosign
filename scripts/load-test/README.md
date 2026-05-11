# Prueba de carga (Fase 5)

## Generar PDFs

Desde la raíz del repo:

```bash
node scripts/gen-load-test-pdfs.mjs --out scripts/load-test/pdfs --count 100 --mb 10
```

Esto crea ~100 ficheros `load-0001.pdf` … con cabecera `%PDF` y tamaño cercano al objetivo (el stream interno es relleno de espacios; válido para la firma PAdES que lee el PDF completo).

Opciones:

| Opción | Descripción |
|--------|-------------|
| `--out` | Carpeta de salida |
| `--count` | Número de archivos (1–10000) |
| `--mb` | MiB aproximados por archivo |
| `--bytes` | Tamaño objetivo en bytes (prioridad sobre `--mb`) |

## Tiempo máximo del lote (servidor)

La ventana de caducidad del intent encolado y del trabajo en SQLite es la misma política, configurable:

```bash
export NEXOSIGN_BATCH_JOB_MAX_SECS=7200   # 2 h; ejemplo para 100 × ~10 MiB
```

Valor por defecto si no se define: **300** (5 min). Para la prueba 100×10 MiB suele hacer falta **≥ 3600** según hardware y token.

## Ejecutar la prueba

1. Arranca NexoSign (`npm run tauri dev` o el binario instalado).
2. En **Firmar**, añade la carpeta que contiene los PDF generados (rutas absolutas).
3. Opcional: anota **tiempo total**, **tiempo por archivo** (aprox.) y **RSS** del proceso (`ps`, Monitor de actividad, Administrador de tareas).

## Cliente HTTP

Las peticiones largas pueden necesitar timeout alto en `curl` u otro cliente:

```bash
curl --max-time 7200 ...
```

## Memoria

NexoSign lee cada PDF completo en RAM al firmar (~orden del tamaño del archivo por documento en cola secuencial). Para ~10 MiB por fichero el pico esperable es modesto frente a un escenario que cargue todo el lote a la vez.
