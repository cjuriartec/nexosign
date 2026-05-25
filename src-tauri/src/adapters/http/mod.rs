pub mod local_api_ops;
pub mod openapi;
pub mod state;

pub use crate::domain::pending_batch_intent::PendingBatchIntent;

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use http::request::Parts;
use bytes::Bytes;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::adapters::http::state::SharedState;
use crate::domain::origin_policy;
use crate::adapters::persistence::queue_store;
use crate::infrastructure::batch_pdf_validation::{
    validate_batch_pdf_inputs,
    validate_pdf_magic_and_size,
    MAX_PDFS_PER_BATCH_INTENT,
    MAX_TOTAL_BATCH_INTENT_BYTES,
};

/// Techo para servir por HTTP un PDF firmado (entrada máx. + margen por firma incrustada).
const MAX_SIGNED_DOWNLOAD_BYTES: u64 =
    crate::infrastructure::batch_pdf_validation::MAX_BATCH_PDF_BYTES + 8 * 1024 * 1024;

pub use crate::infrastructure::local_api_listen::LOCAL_API_DEFAULT_PORT as LOCAL_API_PORT;

/// Límite del cuerpo HTTP para `POST /batch/sign/intent` (multipart puede acercarse a la suma de PDF).
const MAX_BATCH_INTENT_BODY: usize =
    (MAX_TOTAL_BATCH_INTENT_BYTES as usize).saturating_add(512 * 1024);

/// Techo JSON para `POST /api/v1/batch/sign` (cert_id, inputs, pin, sello base64).
const MAX_BATCH_SIGN_BODY: usize = 4 * 1024 * 1024;

pub(crate) fn validate_optional_output_dir(path: Option<std::path::PathBuf>) -> Result<Option<std::path::PathBuf>, String> {
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

fn api_err(status: StatusCode, code: &'static str, detail: impl Into<String>) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": code,
            "detail": detail.into(),
        })),
    )
        .into_response()
}

/// Petición HTTP al propio servidor API en loopback (cabecera `Host`).
fn host_is_loopback_local_api(host: &str, port: u16) -> bool {
    let host = host.trim().to_ascii_lowercase();
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

/// Pestaña abierta sobre esta API (p. ej. Swagger en `/docs`): el GET puede ir sin `Origin`.
fn referer_is_loopback_local_api(referer: &str, port: u16) -> bool {
    let r = referer.trim().to_ascii_lowercase();
    r.starts_with(&format!("http://127.0.0.1:{port}/"))
        || r.starts_with(&format!("http://localhost:{port}/"))
}

/// HTTP/2 usa `:authority`; a veces no hay `Host` en el mapa pero sí autoridad en el [`Uri`].
fn uri_authority_is_loopback_local_api(uri: &Uri, port: u16) -> bool {
    let Some(auth) = uri.authority() else {
        return false;
    };
    let host = auth.host().to_ascii_lowercase();
    if host != "127.0.0.1" && host != "localhost" {
        return false;
    }
    auth.port_u16() == Some(port)
}

fn gate_batch_origin_missing_origin_response(port: u16) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "missing_origin",
            "hint": format!(
                "Los GET desde Swagger a menudo no envían Origin; la API acepta llamadas al puerto local sin Origin si Host/Referer/URI indican 127.0.0.1:{port}. Desde curl: -H \"Origin: http://127.0.0.1:{port}\""
            )
        })),
    )
        .into_response()
}

/// Respaldo cuando el cliente no envía `Origin`/`Referer` (p. ej. fetch a loopback).
static X_CLIENT_ORIGIN: HeaderName = HeaderName::from_static("x-client-origin");

fn request_is_loopback_local_api(headers: &HeaderMap, listen_port: u16) -> bool {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| host_is_loopback_local_api(h, listen_port))
}

/// `Origin`, luego `X-Client-Origin` (solo Host loopback), luego `Referer` (salvo Swagger local).
fn client_origin_from_headers(headers: &HeaderMap, listen_port: u16) -> Option<String> {
    if let Some(o) = origin_policy::origin_from_header(
        headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok()),
    ) {
        return Some(o);
    }

    if request_is_loopback_local_api(headers, listen_port) {
        if let Some(o) = origin_policy::origin_from_header(
            headers
                .get(&X_CLIENT_ORIGIN)
                .and_then(|v| v.to_str().ok()),
        ) {
            return Some(o);
        }
    }

    let referer = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())?;
    if referer_is_loopback_local_api(referer, listen_port) {
        return None;
    }
    origin_policy::origin_from_referer(referer)
}

/// Rutas batch: el origen del cliente debe estar en la lista **si** se envía; si no hay origen,
/// se aceptan peticiones claras al propio listener en loopback (Swagger, HTTP/2).
fn gate_batch_origin(state: &SharedState, headers: &HeaderMap, uri: &Uri) -> Result<(), Response> {
    let listen_port = state.local_api.gate_listen_port();
    if let Some(origin) = client_origin_from_headers(headers, listen_port) {
        let allowed = state
            .origins
            .read()
            .map(|g| g.is_allowed_origin(&origin))
            .unwrap_or(false);

        if allowed {
            return Ok(());
        }

        if let Some(ref h) = state.app_handle {
            crate::infrastructure::window::show_main_window(h);
            let _ = h.emit(
                "origin_trust_request",
                serde_json::json!({ "origin": origin }),
            );
        }

        return Err(
            (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "origin_not_trusted",
                    "origin": origin,
                })),
            )
                .into_response(),
        );
    }

    if headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| host_is_loopback_local_api(h, listen_port))
    {
        return Ok(());
    }

    if headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|r| referer_is_loopback_local_api(r, listen_port))
    {
        return Ok(());
    }

    if uri_authority_is_loopback_local_api(uri, listen_port) {
        return Ok(());
    }

    Err(gate_batch_origin_missing_origin_response(listen_port))
}

fn emit_pending_batch_intents_changed(state: &SharedState, request_id: &str) {
    if let Some(ref h) = state.app_handle {
        crate::infrastructure::window::show_main_window_if_background(h);
        let _ = h.emit(
            "pending_batch_intents_changed",
            serde_json::json!({ "requestId": request_id }),
        );
    }
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
            HeaderName::from_static("x-client-origin"),
        ])
        .allow_credentials(true)
        .allow_origin(AllowOrigin::predicate(
            move |origin: &HeaderValue, _parts: &Parts| {
                let Ok(s) = origin.to_str() else {
                    return false;
                };
                // Reflejar ACAO para cualquier URL de origen bien formada.
                // La autorización real es `is_allowed_origin` en rutas batch.
                if origin_policy::is_well_formed_origin(s) {
                    return true;
                }
                let guard = origins_for_cors.read().ok();
                let Some(guard) = guard else {
                    return false;
                };
                guard.is_allowed_origin(s)
            },
        ));

    Router::new()
        .route("/openapi.json", get(openapi::get_openapi_json))
        .route("/docs", get(openapi::get_api_docs))
        .route("/health", get(get_health))
        .route("/api/v1/ping", post(post_ping))
        .route("/api/v1/focus", post(post_focus))
        .route("/api/v1/origin/check", get(get_origin_check))
        .route("/api/v1/origin/prompt", post(post_origin_prompt))
        .route(
            "/api/v1/batch/sign/intent",
            post(post_batch_sign_intent).layer(DefaultBodyLimit::max(MAX_BATCH_INTENT_BODY)),
        )
        .route(
            "/api/v1/batch/sign/intent/{request_id}/status",
            get(get_batch_sign_intent_status),
        )
        .route(
            "/api/v1/batch/sign",
            post(post_batch_sign).layer(DefaultBodyLimit::max(MAX_BATCH_SIGN_BODY)),
        )
        .route(
            "/api/v1/batch/jobs/{job_id}/status",
            get(get_batch_job_status),
        )
        .route(
            "/api/v1/batch/jobs/{job_id}/signed-files",
            get(get_batch_signed_manifest),
        )
        .route(
            "/api/v1/batch/jobs/{job_id}/files/{file_index}",
            get(download_batch_signed_file),
        )
        .layer(cors)
        .with_state(state)
}

async fn get_health(State(state): State<SharedState>) -> impl IntoResponse {
    Json(local_api_ops::health_payload(&state))
}

async fn post_ping() -> impl IntoResponse {
    Json(local_api_ops::ping_payload())
}

#[derive(Debug, Deserialize, Default)]
struct FocusBody {
    /// Si se indica, abre el asistente de firma para ese `request_id`.
    #[serde(default)]
    intent: Option<String>,
}

#[derive(Debug, Serialize)]
struct FocusResponse {
    ok: bool,
    focused: bool,
}

/// Trae NexoSign al frente (y opcionalmente navega al intent de firma).
#[derive(Debug, Serialize)]
struct OriginCheckResponse {
    origin: Option<String>,
    trusted: bool,
}

/// Comprueba si el `Origin` de la petición está en la lista permitida (CORS batch).
async fn get_origin_check(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let listen_port = state.local_api.gate_listen_port();
    let origin = client_origin_from_headers(&headers, listen_port);

    let Some(ref origin_str) = origin else {
        return Json(OriginCheckResponse {
            origin: None,
            trusted: false,
        });
    };

    let trusted = state
        .origins
        .read()
        .map(|g| g.is_allowed_origin(origin_str))
        .unwrap_or(false);

    Json(OriginCheckResponse {
        origin,
        trusted,
    })
}

#[derive(Debug, Serialize)]
struct OriginPromptResponse {
    trusted: bool,
    prompted: bool,
}

/// Muestra el diálogo nativo de confianza (bloqueante). Usar desde el cliente al autorizar un origen nuevo.
async fn post_origin_prompt(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let listen_port = state.local_api.gate_listen_port();
    let Some(origin) = client_origin_from_headers(&headers, listen_port) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing_origin",
                "detail": "Se requiere Origin o X-Client-Origin (loopback) con la URL del cliente",
            })),
        )
            .into_response();
    };

    let already = state
        .origins
        .read()
        .map(|g| g.is_allowed_origin(&origin))
        .unwrap_or(false);
    if already {
        return Json(OriginPromptResponse {
            trusted: true,
            prompted: false,
        })
        .into_response();
    }

    let state_clone = state.clone();
    let trusted = tokio::task::spawn_blocking(move || {
        let db = state_clone
            .queue_sqlite_path
            .as_deref()
            .map(|p| p.as_path());
        let Some(ref app) = state_clone.app_handle else {
            return false;
        };
        crate::infrastructure::origin_trust_prompt::prompt_client_origin_trust_blocking(
            app,
            &state_clone.origins,
            db,
            &origin,
        )
    })
    .await
    .unwrap_or(false);

    Json(OriginPromptResponse {
        trusted,
        prompted: true,
    })
    .into_response()
}

async fn post_focus(
    State(state): State<SharedState>,
    _headers: HeaderMap,
    body: Option<Json<FocusBody>>,
) -> impl IntoResponse {
    let intent = body
        .map(|Json(b)| b.intent)
        .flatten()
        .filter(|s| !s.trim().is_empty());

    if let Some(ref h) = state.app_handle {
        crate::infrastructure::window::show_main_window(h);
        if let Some(ref request_id) = intent {
            emit_pending_batch_intents_changed(&state, request_id);
        }
        Json(FocusResponse {
            ok: true,
            focused: true,
        })
        .into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "app_unavailable",
                "detail": "NexoSign no está listo para mostrar la ventana.",
            })),
        )
            .into_response()
    }
}

/// Registra PDF para firmar **sin encolar** hasta que el usuario complete el asistente en la app.
/// Acepta `application/json` (rutas locales) o `multipart/form-data` (campos `file`/`files` + `output_dir` opcional).
async fn post_batch_sign_intent(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> impl IntoResponse {
    if let Err(resp) = gate_batch_origin(&state, &headers, &uri) {
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

    if let Some(ref db_arc) = state.queue_sqlite_path {
        if let Err(e) = queue_store::upsert_intent_payload(db_arc.as_ref(), &request_id, &intent) {
            tracing::warn!(
                error = %e,
                request_id = %request_id,
                "persistir intent JSON en SQLite"
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "intent_persist_failed",
                    "detail": e.to_string(),
                })),
            )
                .into_response();
        }
    }

    {
        let mut g = match state.pending_batch_intents.lock() {
            Ok(g) => g,
            Err(_) => {
                return api_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "intent_state_locked",
                    "estado intents bloqueado",
                );
            }
        };
        g.insert(request_id.clone(), intent);
    }

    emit_pending_batch_intents_changed(&state, &request_id);

    Json(BatchSignIntentResponse { request_id }).into_response()
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

    if let Some(ref db_arc) = state.queue_sqlite_path {
        if let Err(e) = queue_store::upsert_intent_payload(db_arc.as_ref(), &request_id, &intent) {
            tracing::warn!(
                error = %e,
                request_id = %request_id,
                "persistir intent multipart en SQLite"
            );
            let _ = std::fs::remove_dir_all(&staging_dir);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "intent_persist_failed",
                    "detail": e.to_string(),
                })),
            )
                .into_response();
        }
    }

    {
        let mut g = match state.pending_batch_intents.lock() {
            Ok(g) => g,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return api_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "intent_state_locked",
                    "estado intents bloqueado",
                );
            }
        };
        g.insert(request_id.clone(), intent);
    }

    emit_pending_batch_intents_changed(&state, &request_id);

    Json(BatchSignIntentResponse { request_id }).into_response()
}

async fn post_batch_sign(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    Json(body): Json<BatchSignBody>,
) -> impl IntoResponse {
    if let Err(resp) = gate_batch_origin(&state, &headers, &uri) {
        return resp;
    }

    match local_api_ops::try_enqueue_batch_sign(&state, body) {
        Ok(r) => Json(r).into_response(),
        Err(e) => (
            e.status,
            Json(serde_json::json!({
                "error": e.code,
                "detail": e.detail,
            })),
        )
            .into_response(),
    }
}

/// Sondeo desde el portal web: `request_id` del intent → fase y `job_id` cuando ya se encoló la firma.
async fn get_batch_sign_intent_status(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = gate_batch_origin(&state, &headers, &uri) {
        return resp;
    }

    let awaiting = {
        let mut g = match state.pending_batch_intents.lock() {
            Ok(g) => g,
            Err(_) => {
                return api_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "intent_state_locked",
                    "estado intents bloqueado",
                );
            }
        };
        match g.get(&request_id) {
            None => false,
            Some(ent) if ent.is_expired() => {
                let staging = ent.staging_dir.clone();
                g.remove(&request_id);
                drop(g);
                if let Some(ref dir) = staging {
                    let _ = std::fs::remove_dir_all(dir);
                }
                if let Some(ref db_arc) = state.queue_sqlite_path {
                    let _ = queue_store::delete_intent_payload(db_arc.as_ref(), &request_id);
                }
                false
            }
            Some(_) => true,
        }
    };

    if awaiting {
        return Json(serde_json::json!({
            "request_id": request_id,
            "phase": "awaiting_confirmation",
            "job_id": serde_json::Value::Null,
        }))
        .into_response();
    }

    let job_id = match state.intent_request_to_job.lock() {
        Ok(g) => match g.get(&request_id).cloned() {
            Some(j) => j,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": "intent_not_found",
                        "detail": "request_id desconocido, expirado o sin encolar firma tras el intent."
                    })),
                )
                    .into_response();
            }
        },
        Err(_) => {
            return api_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "intent_job_map_locked",
                "estado intent_request_to_job bloqueado",
            );
        }
    };

    let manifest_href = format!("/api/v1/batch/jobs/{}/signed-files", job_id);

    let signed_paths_opt = match state.batch_signed_outputs.lock() {
        Ok(g) => g.get(&job_id).cloned(),
        Err(_) => {
            return api_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "batch_outputs_locked",
                "estado batch outputs bloqueado",
            );
        }
    };

    match signed_paths_opt {
        None => Json(serde_json::json!({
            "request_id": request_id,
            "phase": "processing",
            "job_id": job_id,
            "manifest_href": manifest_href,
        }))
        .into_response(),
        Some(paths) => Json(serde_json::json!({
            "request_id": request_id,
            "phase": "completed",
            "job_id": job_id,
            "signed_file_count": paths.len(),
            "manifest_href": manifest_href,
        }))
        .into_response(),
    }
}

/// Estado del trabajo de firma (fase, progreso). **No** figura en `openapi/nexosign-local-api.openapi.json` (contrato externo); lo usa la UI vía fetch local.
async fn get_batch_job_status(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = gate_batch_origin(&state, &headers, &uri) {
        return resp;
    }
    match local_api_ops::try_get_batch_job_snapshot(&state, &job_id) {
        Ok(snap) => Json(snap).into_response(),
        Err(e) => (
            e.status,
            Json(serde_json::json!({
                "error": e.code,
                "detail": e.detail,
            })),
        )
            .into_response(),
    }
}

/// Lista índices y URLs relativas para descargar los PDF firmados de un trabajo terminado.
async fn get_batch_signed_manifest(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = gate_batch_origin(&state, &headers, &uri) {
        return resp;
    }
    let paths_opt = match state.batch_signed_outputs.lock() {
        Ok(g) => g.get(&job_id).cloned(),
        Err(_) => {
            return api_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "batch_outputs_locked",
                "estado batch outputs bloqueado",
            );
        }
    };
    let Some(paths) = paths_opt else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "job_outputs_not_found",
                "detail": "Sin salidas registradas: el trabajo no existe, no ha terminado o el job_id es incorrecto."
            })),
        )
            .into_response();
    };
    let count = paths.len();
    let files: Vec<serde_json::Value> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let filename = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("signed.pdf")
                .to_string();
            serde_json::json!({
                "index": i,
                "filename": filename,
                "href": format!("/api/v1/batch/jobs/{}/files/{}", job_id, i),
            })
        })
        .collect();

    Json(serde_json::json!({
        "job_id": job_id,
        "count": count,
        "files": files,
    }))
    .into_response()
}

/// Descarga un PDF firmado por índice (mismo orden que en `signed-files`).
async fn download_batch_signed_file(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    Path((job_id, file_index)): Path<(String, usize)>,
) -> impl IntoResponse {
    if let Err(resp) = gate_batch_origin(&state, &headers, &uri) {
        return resp;
    }

    let path = {
        let g = match state.batch_signed_outputs.lock() {
            Ok(g) => g,
            Err(_) => {
                return api_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "batch_outputs_locked",
                    "estado batch outputs bloqueado",
                );
            }
        };
        let Some(paths) = g.get(&job_id) else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "job_outputs_not_found" })),
            )
                .into_response();
        };
        if file_index >= paths.len() {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "file_index_out_of_range" })),
            )
                .into_response();
        }
        paths[file_index].clone()
    };

    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "archivo ya no disponible en disco" })),
            )
                .into_response();
        }
    };
    if !meta.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "no es un fichero regular" })),
        )
            .into_response();
    }
    let len = meta.len();
    if len > MAX_SIGNED_DOWNLOAD_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({ "error": "pdf firmado supera el límite de descarga" })),
        )
            .into_response();
    }

    let path_for_read = path.clone();
    let bytes = match tokio::task::spawn_blocking(move || std::fs::read(&path_for_read)).await {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("lectura: {e}") })),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "fallo al leer fichero" })),
            )
                .into_response();
        }
    };

    let fname = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("documento_firmado.pdf");
    let safe_fname: String = fname.chars().filter(|c| !matches!(c, '"' | '\\' | '\r' | '\n')).collect();

    match Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", safe_fname),
        )
        .body(Body::from(bytes))
    {
        Ok(res) => res.into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "respuesta HTTP" })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::worker::batch::BatchJob;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;
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
    async fn batch_gate_accepts_host_on_custom_listen_port_without_origin() {
        use std::sync::Arc;

        use crate::infrastructure::local_api_listen::LocalApiRuntime;

        let local_api = Arc::new(LocalApiRuntime::new());
        local_api.set_listening(15001);
        let mut state = SharedState::test_default();
        state.local_api = local_api;
        let app = build_router(state);

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign/intent")
                    .header("Host", "127.0.0.1:15001")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"inputs":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(
            res.status(),
            StatusCode::FORBIDDEN,
            "Host en el puerto efectivo debe pasar el gate sin Origin"
        );
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
    async fn batch_sign_intent_multipart_stores_staging_dir() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
        let state = SharedState::test_http(Some(tx), Some(pending.clone()));
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
        let app = build_router(SharedState::test_http(Some(tx), Some(pending.clone())));

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
    async fn batch_sign_intent_rejects_unsupported_content_type() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign/intent")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", "text/plain")
                    .body(Body::from("x"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn batch_sign_intent_rejects_empty_inputs_json() {
        let app = build_router(SharedState::test_default());
        let body = serde_json::json!({ "inputs": [] });
        let res = app
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
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn batch_sign_intent_multipart_rejects_missing_boundary() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign/intent")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", "multipart/form-data")
                    .body(Body::from("x"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn batch_sign_intent_multipart_rejects_without_files() {
        let app = build_router(SharedState::test_default());
        let boundary = "nexosign_mp_empty";
        let ct = format!("multipart/form-data; boundary={boundary}");
        let mp_body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"output_dir\"\r\n\r\n\
             /tmp\r\n\
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
    }

    #[tokio::test]
    async fn batch_sign_rejects_empty_cert_id() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(1);
        let app = build_router(SharedState::test_http(Some(tx), None));
        let tmp = std::env::temp_dir().join(format!("nexosign-empty-cert-{}.pdf", std::process::id()));
        std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
        let abs = tmp.canonicalize().unwrap();
        let body = serde_json::json!({
            "cert_id_hex": " ",
            "inputs": [abs.to_str().unwrap()],
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
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn batch_sign_rejects_empty_pin() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(1);
        let app = build_router(SharedState::test_http(Some(tx), None));
        let tmp = std::env::temp_dir().join(format!("nexosign-empty-pin-{}.pdf", std::process::id()));
        std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
        let abs = tmp.canonicalize().unwrap();
        let body = serde_json::json!({
            "cert_id_hex": "aa",
            "pin": "  ",
            "inputs": [abs.to_str().unwrap()],
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
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn batch_sign_rejects_out_of_range_signature_grid() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(1);
        let app = build_router(SharedState::test_http(Some(tx), None));
        let tmp = std::env::temp_dir().join(format!("nexosign-grid-{}.pdf", std::process::id()));
        std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
        let abs = tmp.canonicalize().unwrap();
        let body = serde_json::json!({
            "cert_id_hex": "aa",
            "inputs": [abs.to_str().unwrap()],
            "signature_grid": { "col": 4, "row": 9 }
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
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn batch_sign_returns_503_when_queue_is_full() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BatchJob>(1);
        let dummy = BatchJob {
            job_id: "prefill".into(),
            cert_id_hex: "00".into(),
            inputs: vec![],
            cancel: CancellationToken::new(),
            output_dir: None,
            signature_grid: None,
            pin: None,
            seal_png: None,
            cleanup_paths: vec![],
        };
        tx.try_send(dummy).expect("prefill queue");
        let app = build_router(SharedState::test_http(Some(tx), None));

        let tmp = std::env::temp_dir().join(format!("nexosign-full-{}.pdf", std::process::id()));
        std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
        let abs = tmp.canonicalize().unwrap();
        let body = serde_json::json!({
            "cert_id_hex": "aa",
            "inputs": [abs.to_str().unwrap()],
            "job_id": "job-will-fail-full"
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
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        let _ = rx.try_recv();
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn intent_status_expired_yields_404_intent_not_found() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        use crate::ports::queue_max_wall_clock_secs;

        let pending = Arc::new(Mutex::new(HashMap::new()));
        let tmp = std::env::temp_dir().join(format!(
            "nexosign-exp-intent-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
        let abs = tmp.canonicalize().unwrap();
        let old_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(queue_max_wall_clock_secs() + 60);
        {
            let mut g = pending.lock().unwrap();
            g.insert(
                "expired-rid".into(),
                PendingBatchIntent::restore_from_storage(vec![abs], None, None, old_unix),
            );
        }
        let state = SharedState::test_http(None, Some(pending.clone()));
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/sign/intent/expired-rid/status")
                    .header("Origin", "http://localhost:1420")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "intent_not_found");
        assert!(pending.lock().unwrap().get("expired-rid").is_none());
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn intent_json_persist_failure_returns_500() {
        let dir = std::env::temp_dir().join(format!(
            "nexosign-sqlite-dir-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let tmp = std::env::temp_dir().join(format!(
            "nexosign-persist-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
        let abs = tmp.canonicalize().unwrap();

        let mut state = SharedState::test_http(None, None);
        state.queue_sqlite_path = Some(Arc::new(dir.clone()));
        let app = build_router(state);

        let body = serde_json::json!({
            "inputs": [abs.to_str().unwrap()],
        });
        let res = app
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
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "intent_persist_failed");
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn batch_origin_not_trusted_returns_403_json() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/jobs/some-id/status")
                    .header("Origin", "https://untrusted.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "origin_not_trusted");
    }

    #[tokio::test]
    async fn batch_job_status_missing_origin_403() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/jobs/j1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "missing_origin");
    }

    #[tokio::test]
    async fn batch_job_status_swagger_origin_allowed() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/jobs/j1/status")
                    .header("Origin", "http://127.0.0.1:14500")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn batch_job_status_loopback_host_without_origin_ok() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/jobs/j1/status")
                    .header("Host", "127.0.0.1:14500")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn batch_job_status_referer_docs_without_origin_ok() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/jobs/j1/status")
                    .header("Referer", "http://127.0.0.1:14500/docs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn batch_job_status_uri_authority_http2_style_without_origin_ok() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("http://127.0.0.1:14500/api/v1/batch/jobs/j1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn signed_manifest_unknown_job_404() {
        let app = build_router(SharedState::test_default());
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/jobs/unknown-job/signed-files")
                    .header("Origin", "http://localhost:1420")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "job_outputs_not_found");
    }

    #[tokio::test]
    async fn download_signed_file_index_out_of_range() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let outs = Arc::new(Mutex::new(HashMap::new()));
        outs
            .lock()
            .unwrap()
            .insert("job1".into(), vec![std::path::PathBuf::from("/tmp/a.pdf")]);
        let mut state = SharedState::test_default();
        state.batch_signed_outputs = outs;
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/jobs/job1/files/99")
                    .header("Origin", "http://localhost:1420")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "file_index_out_of_range");
    }

    #[tokio::test]
    async fn download_signed_file_rejects_over_limit_bytes() {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let dir = std::env::temp_dir().join(format!("nexosign-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pdf_path = dir.join("big.pdf");
        let f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&pdf_path)
            .unwrap();
        f.set_len(MAX_SIGNED_DOWNLOAD_BYTES + 1024).unwrap();
        drop(f);

        let outs = Arc::new(Mutex::new(HashMap::new()));
        outs.lock().unwrap().insert("job-big".into(), vec![pdf_path]);
        let mut state = SharedState::test_default();
        state.batch_signed_outputs = outs;
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/batch/jobs/job-big/files/0")
                    .header("Origin", "http://localhost:1420")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn batch_sign_body_over_default_limit_is_rejected() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
        let app = build_router(SharedState::test_http(Some(tx), None));
        let big = vec![b'x'; MAX_BATCH_SIGN_BODY + 2048];
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/batch/sign")
                    .header("Origin", "http://localhost:1420")
                    .header("Content-Type", "application/json")
                    .body(Body::from(big))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            res.status() == StatusCode::PAYLOAD_TOO_LARGE || res.status() == StatusCode::BAD_REQUEST,
            "unexpected status {}",
            res.status()
        );
    }
}
