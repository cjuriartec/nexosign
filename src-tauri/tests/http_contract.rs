//! Contrato HTTP del binario (tests de integración en `tests/`).

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use nexosign_lib::adapters::http::state::SharedState;
use nexosign_lib::adapters::http::{build_router, LOCAL_API_PORT};
use tower::ServiceExt;

#[tokio::test]
async fn integration_health_echoes_version_and_port_constant() {
    assert_eq!(LOCAL_API_PORT, 14500);
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
    assert_eq!(v["service"], "nexosign");
}
