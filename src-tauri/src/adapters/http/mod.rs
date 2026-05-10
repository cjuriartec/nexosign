pub mod state;

mod pending_batch_intent;

pub use pending_batch_intent::{PendingBatchIntent, PENDING_INTENT_TTL_SECS};

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use http::request::Parts;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::adapters::http::state::{HealthResponse, PingResponse, SharedState};
use crate::adapters::pdf::pades::SignatureGridPlacement;
use crate::adapters::worker::batch::BatchJob;
use crate::infrastructure::batch_pdf_validation::validate_batch_pdf_inputs;

pub const LOCAL_API_PORT: u16 = 14500;

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
        .route("/api/v1/batch/sign/intent", post(post_batch_sign_intent))
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

/// Registra rutas de PDF para firmar **sin encolar** hasta que el usuario complete el asistente en la app.
async fn post_batch_sign_intent(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<BatchSignIntentBody>,
) -> impl IntoResponse {
    if let Err(resp) = gate_batch_origin(&state, &headers) {
        return resp;
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

    let request_id = uuid::Uuid::new_v4().to_string();
    let intent = PendingBatchIntent::new(body.inputs, output_dir);

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

    let job = BatchJob {
        job_id: job_id.clone(),
        cert_id_hex: body.cert_id_hex,
        inputs: body.inputs,
        cancel,
        output_dir,
        signature_grid,
        pin: pin_for_worker,
        seal_png,
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
