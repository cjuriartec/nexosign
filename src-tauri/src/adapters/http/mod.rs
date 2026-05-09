use axum::{
    extract::State,
    http::{HeaderValue, Method, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use http::request::Parts;
use serde::Deserialize;
use tauri::Emitter;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::adapters::http::state::{HealthResponse, PingResponse, SharedState};

pub const LOCAL_API_PORT: u16 = 14500;

pub fn build_router(state: SharedState) -> Router {
    let origins_for_cors = state.origins.clone();
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(tower_http::cors::Any)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
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
    async fn cors_blocks_unknown_origin_for_actual_request() {
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
        // Sin Origin permitido, tower-http puede responder 403 u omitir ACAO según versión.
        assert_ne!(res.status(), StatusCode::OK);
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
}
