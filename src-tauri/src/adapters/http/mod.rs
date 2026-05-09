pub mod state;

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use http::request::Parts;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::adapters::http::state::{HealthResponse, PingResponse, SharedState};
use crate::adapters::pdf::pades::SignatureGridPlacement;
use crate::adapters::worker::batch::BatchJob;

pub const LOCAL_API_PORT: u16 = 14500;

/// Tamaño máximo por PDF en un lote (50 MiB).
const MAX_BATCH_PDF_BYTES: u64 = 50 * 1024 * 1024;

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

fn validate_batch_inputs(paths: &[std::path::PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("inputs no puede estar vacío".into());
    }
    for p in paths {
        if !p.is_absolute() {
            return Err(format!(
                "cada ruta debe ser absoluta (recibido: {})",
                p.display()
            ));
        }
        let meta = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if !meta.is_file() {
            return Err(format!("no es un archivo regular: {}", p.display()));
        }
        if meta.len() > MAX_BATCH_PDF_BYTES {
            return Err(format!(
                "archivo demasiado grande (máx. 50 MiB): {}",
                p.display()
            ));
        }
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
        if !ext.eq_ignore_ascii_case("pdf") {
            return Err(format!("solo se admiten .pdf: {}", p.display()));
        }
    }
    Ok(())
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
    /// Primera página: casilla 7×5 (col 0–6, row 0–4; fila 0 = cabecera del PDF).
    #[serde(default)]
    pub signature_grid: Option<SignatureGridDto>,
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

    if let Err(msg) = validate_batch_inputs(&body.inputs) {
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
            if g.col > 6 || g.row > 4 {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(
                        serde_json::json!({ "error": "signature_grid: col debe ser 0–6 y row 0–4 (rejilla 7×5)" }),
                    ),
                )
                    .into_response();
            }
            Some(SignatureGridPlacement { col: g.col, row: g.row })
        }
        None => None,
    };

    if let Some(ref pin_raw) = body.pin {
        let pin_trim = pin_raw.trim();
        if !pin_trim.is_empty() {
            let Some(mgr) = state.pkcs11.clone() else {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({ "error": "PIN vía API no disponible (modo prueba o sin token)" })),
                )
                    .into_response();
            };
            let cert_hex = body.cert_id_hex.trim().to_string();
            let pin_owned = pin_trim.to_string();
            let login_res =
                tokio::task::spawn_blocking(move || mgr.login_for_certificate(pin_owned, &cert_hex)).await;
            match login_res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({ "error": e.to_string() })),
                    )
                        .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e.to_string() })),
                    )
                        .into_response();
                }
            }
        }
    }

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
    };

    match tx.try_send(job) {
        Ok(()) => Json(BatchSignResponse {
            job_id,
            queued: true,
        })
        .into_response(),
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
}
