pub mod state;

mod pending_batch_intent;

pub use pending_batch_intent::{PendingBatchIntent, PENDING_INTENT_TTL_SECS};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use http::request::Parts;
use base64::Engine;
use bytes::Bytes;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::adapters::http::state::{HealthResponse, PingResponse, SharedState};
use crate::adapters::pdf::pades::SignatureGridPlacement;
use crate::adapters::worker::batch::BatchJob;
use crate::infrastructure::batch_pdf_validation::{
    validate_batch_pdf_inputs,
    validate_pdf_magic_and_size,
    MAX_PDFS_PER_BATCH_INTENT,
    MAX_TOTAL_BATCH_INTENT_BYTES,
};

pub const LOCAL_API_PORT: u16 = 14500;

/// Límite del cuerpo HTTP para `POST /batch/sign/intent` (multipart puede acercarse a la suma de PDF).
const MAX_BATCH_INTENT_BODY: usize =
    (MAX_TOTAL_BATCH_INTENT_BYTES as usize).saturating_add(512 * 1024);

fn validate_optional_output_dir(path: Option<std::path::PathBuf>) -> Result<Option<std::path::PathBuf>, String> {
    let Some(p) = path else {
        return Ok(None);
    };
    if !p.is_absolute() {
        return Err(format!(
            "output_dir debe ser ruta absoluta (recibido: {})",
            p.display()
        ));
    }
    std::fs::create_dir_all(&p).map_err(|e| format!("output_dir: {e}"))?;
    Ok(Some(p))
}

/// Exige `Origin` conocido (lista CORS / SQLite) antes de encolar firma batch.
fn gate_batch_origin(state: &SharedState, headers: &HeaderMap) -> Result<(), Response> {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return Err(
            (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "missing_origin",
                    "hint": "El navegador debe enviar el header Origin; para curl use -H \"Origin: http://localhost:1420\""
                })),
            )
                .into_response(),
        );
    };

    let allowed = state
        .origins
        .read()
        .map(|g| g.is_allowed_origin(origin))
        .unwrap_or(false);

    if allowed {
        return Ok(());
    }

    if let Some(ref h) = state.app_handle {
        let _ = h.emit(
            "origin_trust_request",
            serde_json::json!({ "origin": origin }),
        );
    }

    Err(
        (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "origin_not_trusted",
                "origin": origin,
            })),
        )
            .into_response(),
    )
}

#[derive(Debug, Deserialize)]
pub struct BatchSignIntentBody {
    pub inputs: Vec<std::path::PathBuf>,
    #[serde(default)]
    pub output_dir: Option<std::path::PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct BatchSignIntentResponse {
    pub request_id: String,
    /// Abrir en el sistema para lanzar NexoSign y el asistente de firma.
    pub deep_link: String,
}

#[derive(Debug, Deserialize)]
pub struct SignatureGridDto {
    pub col: u8,
    pub row: u8,
}

#[derive(Debug, Deserialize)]
pub struct BatchSignBody {
    pub cert_id_hex: String,
    pub inputs: Vec<std::path::PathBuf>,
    #[serde(default)]
    pub job_id: Option<String>,
    /// Solo loopback; la app desbloquea el token antes de encolar.
    #[serde(default)]
    pub pin: Option<String>,
    /// Directorio absoluto donde escribir `{stem}_firmado.pdf` para cada entrada (p. ej. carpeta `…_firmados`).
    #[serde(default)]
    pub output_dir: Option<std::path::PathBuf>,
    /// Primera página: casilla en rejilla **3 columnas × 5 filas** (col 0–2, row 0–4).
    #[serde(default)]
    pub signature_grid: Option<SignatureGridDto>,
    /// Consumido si la firma viene de `POST /api/v1/batch/sign/intent`.
    #[serde(default)]
    pub intent_request_id: Option<String>,
    /// PNG del sello (base64 estándar), mismo aspecto que Certificados — opcional.
    #[serde(default)]
    pub signature_seal_png_base64: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchSignResponse {
    pub job_id: String,
    pub queued: bool,
}

pub fn build_router(state: SharedState) -> Router {
    let origins_for_cors = state.origins.clone();
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
        ])
        .allow_credentials(true)
        .allow_origin(AllowOrigin::predicate(
            move |origin: &HeaderValue, _parts: &Parts| {
                let Ok(s) = origin.to_str() else {
                    return false;
                };
                let guard = origins_for_cors.read().ok();
                let Some(guard) = guard else {
                    return false;
                };
                guard.is_allowed_origin(s)
            },
        ));

    Router::new()
        .route("/health", get(get_health))
        .route("/api/v1/ping", post(post_ping))
        .route("/api/v1/demo-progress", post(post_demo_progress))
        .route(
            "/api/v1/batch/sign/intent",
            post(post_batch_sign_intent).layer(DefaultBodyLimit::max(MAX_BATCH_INTENT_BODY)),
        )
        .route("/api/v1/batch/sign", post(post_batch_sign))
        .layer(cors)
        .with_state(state)
}

async fn get_health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        service: "nexosign",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn post_ping() -> impl IntoResponse {
    Json(PingResponse { ok: true })
}

#[derive(Deserialize)]
pub struct DemoProgressPayload {
    #[serde(default)]
    pub job_id: Option<String>,
}

/// Emite un evento `progreso` al frontend (stub fase 1). Útil para validar el canal IPC.
async fn post_demo_progress(
    State(state): State<SharedState>,
    Json(body): Json<DemoProgressPayload>,
) -> impl IntoResponse {
    let Some(handle) = state.app_handle.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "app_handle not available (test mode)" })),
        )
            .into_response();
    };

    let payload = serde_json::json!({
        "actual": 1,
        "total": 10,
        "job_id": body.job_id.unwrap_or_else(|| "demo".to_string()),
    });

    if let Err(e) = handle.emit("progreso", &payload) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    Json(serde_json::json!({ "emitted": true })).into_response()
}

/// Registra PDF para firmar **sin encolar** hasta que el usuario complete el asistente en la app.
/// Acepta `application/json` (rutas locales) o `multipart/form-data` (campos `file`/`files` + `output_dir` opcional).
async fn post_batch_sign_intent(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Body,
) -> impl IntoResponse {
    if let Err(resp) = gate_batch_origin(&state, &headers) {
        return resp;
    }

    let ctype = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if ctype.starts_with("multipart/form-data") {
        return post_batch_sign_intent_multipart(state, headers, body).await;
    }

    if !ctype.starts_with("application/json") && !ctype.is_empty() {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(serde_json::json!({
                "error": "Content-Type debe ser application/json o multipart/form-data"
            })),
        )
            .into_response();
    }

    let bytes = match axum::body::to_bytes(body, MAX_BATCH_INTENT_BODY).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("lectura del cuerpo: {e}") })),
            )
                .into_response();
        }
    };

    let json_body: BatchSignIntentBody = match serde_json::from_slice(&bytes) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("JSON inválido: {e}") })),
            )
                .into_response();
        }
    };

    post_batch_sign_intent_json(state, json_body).await
}

async fn post_batch_sign_intent_json(
    state: SharedState,
    body: BatchSignIntentBody,
) -> Response {
    if let Err(msg) = validate_batch_pdf_inputs(&body.inputs) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
    }

    let output_dir = match validate_optional_output_dir(body.output_dir) {
        Ok(d) => d,
        Err(msg) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let intent = PendingBatchIntent::new(body.inputs, output_dir, None);

    {
        let mut g = match state.pending_batch_intents.lock() {
            Ok(g) => g,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "estado intents bloqueado" })),
                )
                    .into_response();
            }
        };
        g.insert(request_id.clone(), intent);
    }

    let deep_link = format!("nexosign://sign?intent={}", request_id);

    Json(BatchSignIntentResponse {
        request_id,
        deep_link,
    })
    .into_response()
}

fn staging_root_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("nexosign-intent-uploads")
}

fn sanitize_staged_filename(original: &str, index: usize) -> String {
    let base = std::path::Path::new(original)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let safe: String = base
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
        .collect();
    let stem = safe.trim();
    let stem = if stem.is_empty() {
        format!("document_{index}")
    } else {
        stem.to_string()
    };
    if stem.to_lowercase().ends_with(".pdf") {
        stem
    } else {
        format!("{stem}.pdf")
    }
}

fn unique_path_in_dir(dir: &std::path::Path, filename: &str) -> std::path::PathBuf {
    let mut p = dir.join(filename);
    if !p.exists() {
        return p;
    }
    let stem = std::path::Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into());
    for i in 2_u16..1000 {
        p.set_file_name(format!("{stem}_{i}.pdf"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!(
        "{}_{}.pdf",
        stem,
        uuid::Uuid::new_v4().simple()
    ))
}

async fn post_batch_sign_intent_multipart(
    state: SharedState,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let ctype = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let boundary = match multer::parse_boundary(ctype) {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "multipart sin boundary válido en Content-Type"
                })),
            )
                .into_response();
        }
    };

    let bytes = match axum::body::to_bytes(body, MAX_BATCH_INTENT_BODY).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("lectura del cuerpo: {e}") })),
            )
                .into_response();
        }
    };

    let staging_root = staging_root_dir();
    if let Err(e) = std::fs::create_dir_all(&staging_root) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("staging: {e}") })),
        )
            .into_response();
    }

    let request_id = uuid::Uuid::new_v4().to_string();
    let staging_dir = staging_root.join(&request_id);
    if let Err(e) = std::fs::create_dir(&staging_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("staging: {e}") })),
        )
            .into_response();
    }

    let stream = stream::once(async move { Ok::<Bytes, std::convert::Infallible>(bytes) });
    let mut multipart = multer::Multipart::new(stream, boundary);

    let mut output_dir_text: Option<String> = None;
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    let mut total_bytes: u64 = 0;
    let mut file_index: usize = 0;

    loop {
        let field = match multipart.next_field().await {
            Ok(f) => f,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("multipart: {e}") })),
                )
                    .into_response();
            }
        };
        let Some(field) = field else {
            break;
        };

        let name = field.name().unwrap_or("");

        if name == "output_dir" {
            match field.text().await {
                Ok(t) => output_dir_text = Some(t),
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&staging_dir);
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("output_dir: {e}") })),
                    )
                        .into_response();
                }
            }
            continue;
        }

        if name != "files" && name != "file" {
            continue;
        }

        let filename_hint = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let data = match field.bytes().await {
            Ok(d) => d,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("lectura de fichero: {e}") })),
                )
                    .into_response();
            }
        };

        let len = data.len() as u64;
        if paths.len() >= MAX_PDFS_PER_BATCH_INTENT {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("demasiados PDF (máx. {MAX_PDFS_PER_BATCH_INTENT})")
                })),
            )
                .into_response();
        }

        let next_total = total_bytes.saturating_add(len);
        if next_total > MAX_TOTAL_BATCH_INTENT_BYTES {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "suma de tamaños de PDF supera el límite del lote"
                })),
            )
                .into_response();
        }

        let prefix_len = std::cmp::min(data.len(), 16);
        let prefix = &data[..prefix_len];
        if let Err(msg) = validate_pdf_magic_and_size(len, prefix) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
        }

        let staged_name = sanitize_staged_filename(&filename_hint, file_index);
        let dest = unique_path_in_dir(&staging_dir, &staged_name);
        file_index += 1;

        if let Err(e) = std::fs::write(&dest, &data) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("escritura temporal: {e}") })),
            )
                .into_response();
        }

        total_bytes = next_total;
        paths.push(dest);
    }

    if paths.is_empty() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "no se recibió ningún PDF (campos file o files)"
            })),
        )
            .into_response();
    }

    let output_dir = match output_dir_text {
        None => None,
        Some(ref s) if s.trim().is_empty() => None,
        Some(s) => match validate_optional_output_dir(Some(std::path::PathBuf::from(s.trim()))) {
            Ok(d) => d,
            Err(msg) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
            }
        },
    };

    let intent = PendingBatchIntent::new(paths, output_dir, Some(staging_dir.clone()));

    {
        let mut g = match state.pending_batch_intents.lock() {
            Ok(g) => g,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "estado intents bloqueado" })),
                )
                    .into_response();
            }
        };
        g.insert(request_id.clone(), intent);
    }

    let deep_link = format!("nexosign://sign?intent={}", request_id);

    Json(BatchSignIntentResponse {
        request_id,
        deep_link,
    })
    .into_response()
}

async fn post_batch_sign(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<BatchSignBody>,
) -> impl IntoResponse {
    let Some(tx) = state.batch_tx.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "cola batch no configurada" })),
        )
            .into_response();
    };

    if let Err(resp) = gate_batch_origin(&state, &headers) {
        return resp;
    }

    if body.cert_id_hex.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "cert_id_hex requerido" })),
        )
            .into_response();
    }

    if let Err(msg) = validate_batch_pdf_inputs(&body.inputs) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
    }

    let output_dir = match validate_optional_output_dir(body.output_dir) {
        Ok(d) => d,
        Err(msg) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };

    let signature_grid = match body.signature_grid {
        Some(g) => {
            if g.col > 2 || g.row > 4 {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(
                        serde_json::json!({ "error": "signature_grid: col debe ser 0–2 y row 0–4 (rejilla 3×5)" }),
                    ),
                )
                    .into_response();
            }
            Some(SignatureGridPlacement { col: g.col, row: g.row })
        }
        None => None,
    };

    if let Some(ref pin_raw) = body.pin {
        if pin_raw.trim().is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "PIN vacío" })),
            )
                .into_response();
        }
        // La validación real del PIN se hace en el worker (spawn_blocking) para
        // que C_Initialize, C_Login y C_Sign ocurran en el mismo hilo OS.
    }

    let pin_for_worker = body
        .pin
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let seal_png: Option<Vec<u8>> = match body.signature_seal_png_base64.as_ref() {
        None => None,
        Some(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                match base64::engine::general_purpose::STANDARD.decode(t) {
                    Ok(raw) if raw.len() <= 1_500_000 => Some(raw),
                    Ok(_) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "error": "signature_seal_png_base64 supera 1,5 MiB"
                            })),
                        )
                            .into_response();
                    }
                    Err(_) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "error": "signature_seal_png_base64 no es base64 válido"
                            })),
                        )
                            .into_response();
                    }
                }
            }
        }
    };

    let job_id = body
        .job_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let cancel = CancellationToken::new();
    {
        let mut g = match state.batch_cancel.lock() {
            Ok(g) => g,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "estado batch bloqueado" })),
                )
                    .into_response();
            }
        };
        g.insert(job_id.clone(), cancel.clone());
    }

    let mut cleanup_paths: Vec<std::path::PathBuf> = Vec::new();
    if let Some(ref rid) = body.intent_request_id {
        if let Ok(g) = state.pending_batch_intents.lock() {
            if let Some(ent) = g.get(rid) {
                if let Some(ref d) = ent.staging_dir {
                    cleanup_paths.push(d.clone());
                }
            }
        }
    }

    let job = BatchJob {
        job_id: job_id.clone(),
        cert_id_hex: body.cert_id_hex,
        inputs: body.inputs,
        cancel,
        output_dir,
        signature_grid,
        pin: pin_for_worker,
        seal_png,
        cleanup_paths,
    };

    match tx.try_send(job) {
        Ok(()) => {
            if let Some(rid) = body.intent_request_id.clone() {
                if let Ok(mut p) = state.pending_batch_intents.lock() {
                    p.remove(&rid);
                }
            }
            Json(BatchSignResponse {
                job_id,
                queued: true,
            })
            .into_response()
        }
        Err(tokio::sync::mpsc::error::TrySendError::Full(j)) => {
            if let Ok(mut g) = state.batch_cancel.lock() {
                g.remove(&j.job_id);
            }
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "cola batch llena, reintente" })),
            )
                .into_response()
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(j)) => {
            if let Ok(mut g) = state.batch_cancel.lock() {
                g.remove(&j.job_id);
            }
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "cola batch cerrada" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::header;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_ok_without_origin() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["service"], "nexosign");
    }

    #[tokio::test]
    async fn cors_preflight_allowed_for_dev_origin() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/health")
                    .header(
                        "Origin",
                        "http://localhost:1420",
                    )
                    .header("Access-Control-Request-Method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cors_unknown_origin_gets_no_allow_origin_header() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header("Origin", "https://malicious.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // tower-http puede seguir devolviendo 200 al handler; lo importante es no exponer ACAO.
        let acao = res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN);
        assert!(
            acao.is_none(),
            "origen no listado no debe recibir Access-Control-Allow-Origin"
        );
    }

    #[tokio::test]
    async fn ping_returns_ok() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ping")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn batch_sign_enqueue_returns_200_and_job_id() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
        let app = build_router(SharedState::test_with_batch(tx));

        let tmp = std::env::temp_dir().join(format!(
            "nexosign-batch-test-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&tmp, b"%PDF-1.1\n").unwrap();

        let abs = tmp.canonicalize().unwrap();
        let body = serde_json::json!({
            "cert_id_hex": "01ab",
            "inputs": [abs.to_str().unwrap()],
            "job_id": "job-contract-1"
        });

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["queued"], true);
        assert_eq!(v["job_id"], "job-contract-1");

        let job = rx.try_recv().expect("job encolado");
        assert_eq!(job.job_id, "job-contract-1");
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn batch_sign_intent_stores_pending_and_returns_deep_link() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
        let state = SharedState::test_with_batch_intents(tx, pending.clone());
        let app = build_router(state);

        let tmp = std::env::temp_dir().join(format!(
            "nexosign-intent-test-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
        let abs = tmp.canonicalize().unwrap();

        let body = serde_json::json!({
            "inputs": [abs.to_str().unwrap()],
        });

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign/intent")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let request_id = v["request_id"].as_str().unwrap();
        assert!(!request_id.is_empty());
        let deep_link = v["deep_link"].as_str().unwrap();
        assert_eq!(
            deep_link,
            format!("nexosign://sign?intent={}", request_id)
        );

        let guard = pending.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert!(guard.contains_key(request_id));
        drop(guard);
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn batch_sign_intent_multipart_stores_staging_dir() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
        let state = SharedState::test_with_batch_intents(tx, pending.clone());
        let app = build_router(state);

        let boundary = "nexosign_mp_test";
        let ct = format!("multipart/form-data; boundary={boundary}");
        let mp_body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"files\"; filename=\"doc.pdf\"\r\n\
             Content-Type: application/pdf\r\n\r\n\
             %PDF-1.4\n%\r\n\
             --{boundary}--\r\n"
        );

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign/intent")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", &ct)
                    .body(Body::from(mp_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let request_id = v["request_id"].as_str().unwrap();

        let guard = pending.lock().unwrap();
        assert_eq!(guard.len(), 1);
        let ent = guard.get(request_id).expect("intent");
        assert!(ent.staging_dir.is_some());
        let staging = ent.staging_dir.clone().unwrap();
        assert!(staging.join("doc.pdf").is_file() || staging.read_dir().unwrap().count() >= 1);
        drop(guard);
        let _ = std::fs::remove_dir_all(&staging);
    }

    #[tokio::test]
    async fn batch_sign_intent_multipart_rejects_invalid_pdf_magic() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
        let app = build_router(SharedState::test_with_batch_intents(tx, pending.clone()));

        let boundary = "nexosign_mp_bad";
        let ct = format!("multipart/form-data; boundary={boundary}");
        let mp_body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"x.pdf\"\r\n\
             Content-Type: application/pdf\r\n\r\n\
             NOT_A_PDF\r\n\
             --{boundary}--\r\n"
        );

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign/intent")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", &ct)
                    .body(Body::from(mp_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(pending.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn batch_sign_with_intent_request_id_clears_pending_after_enqueue() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
        let state = SharedState::test_with_batch_intents(tx, pending.clone());
        let app = build_router(state);

        let tmp = std::env::temp_dir().join(format!(
            "nexosign-intent-clear-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
        let abs = tmp.canonicalize().unwrap();

        let intent_body = serde_json::json!({
            "inputs": [abs.to_str().unwrap()],
        });

        let res_intent = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign/intent")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", "application/json")
                    .body(Body::from(intent_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res_intent.status(), StatusCode::OK);
        let intent_bytes = to_bytes(res_intent.into_body(), usize::MAX)
            .await
            .unwrap();
        let intent_v: serde_json::Value = serde_json::from_slice(&intent_bytes).unwrap();
        let request_id = intent_v["request_id"].as_str().unwrap().to_string();

        assert_eq!(pending.lock().unwrap().len(), 1);

        let batch_body = serde_json::json!({
            "cert_id_hex": "01ff",
            "inputs": [abs.to_str().unwrap()],
            "job_id": "job-after-intent",
            "intent_request_id": request_id,
        });

        let res_batch = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", "application/json")
                    .body(Body::from(batch_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res_batch.status(), StatusCode::OK);
        let _job = rx.try_recv().expect("job tras intent");

        assert!(
            pending.lock().unwrap().is_empty(),
            "la intención debe borrarse al encolar bien"
        );

        let _ = std::fs::remove_file(&tmp);
    }
}
