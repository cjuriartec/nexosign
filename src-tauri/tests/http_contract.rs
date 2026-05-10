//! Contrato HTTP del binario (tests de integración en `tests/`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use nexosign_lib::adapters::http::state::SharedState;
use nexosign_lib::adapters::http::{build_router, LOCAL_API_PORT};
use nexosign_lib::adapters::worker::batch::BatchJob;
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

#[tokio::test]
async fn integration_batch_sign_returns_queued_and_enqueues_job() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
    let app = build_router(SharedState::test_with_batch(tx));

    let tmp = std::env::temp_dir().join(format!(
        "nexosign-http-contract-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
    let abs = tmp.canonicalize().unwrap();

    let body = serde_json::json!({
        "cert_id_hex": "01ff",
        "inputs": [abs.to_str().unwrap()],
        "job_id": "contract-batch-1"
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
    assert_eq!(v["job_id"], "contract-batch-1");

    let job = rx.try_recv().expect("job en cola");
    assert_eq!(job.job_id, "contract-batch-1");
    let _ = std::fs::remove_file(&tmp);
}

#[tokio::test]
async fn integration_batch_sign_intent_registers_deep_link_shape() {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
    let app = build_router(SharedState::test_with_batch_intents(tx, pending.clone()));

    let tmp = std::env::temp_dir().join(format!(
        "nexosign-http-contract-intent-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
    let abs = tmp.canonicalize().unwrap();

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

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let rid = v["request_id"].as_str().unwrap();
    assert_eq!(
        v["deep_link"].as_str().unwrap(),
        format!("nexosign://sign?intent={}", rid)
    );
    assert_eq!(pending.lock().unwrap().len(), 1);
    let _ = std::fs::remove_file(&tmp);
}

#[tokio::test]
async fn integration_batch_sign_intent_multipart_registers_deep_link_shape() {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
    let app = build_router(SharedState::test_with_batch_intents(tx, pending));

    let boundary = "contract_mp_ok";
    let ct = format!("multipart/form-data; boundary={boundary}");
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"files\"; filename=\"a.pdf\"\r\n\
         Content-Type: application/pdf\r\n\r\n\
         %PDF-1.4\n\r\n\
         --{boundary}--\r\n"
    );

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/batch/sign/intent")
                .header("Origin", "http://localhost:1420")
                .header("Content-Type", ct)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let rid = v["request_id"].as_str().unwrap();
    assert_eq!(v["deep_link"], format!("nexosign://sign?intent={rid}"));
}

#[tokio::test]
async fn integration_batch_sign_intent_multipart_invalid_pdf_returns_bad_request() {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let (tx, _rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
    let app = build_router(SharedState::test_with_batch_intents(tx, pending));

    let boundary = "contract_mp_bad";
    let ct = format!("multipart/form-data; boundary={boundary}");
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"x.pdf\"\r\n\
         Content-Type: application/pdf\r\n\r\n\
         NOTPDF\r\n\
         --{boundary}--\r\n"
    );

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/batch/sign/intent")
                .header("Origin", "http://localhost:1420")
                .header("Content-Type", ct)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn integration_intent_then_batch_enqueues_with_intent_request_id() {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<BatchJob>(4);
    let app = build_router(SharedState::test_with_batch_intents(tx, pending.clone()));

    let tmp = std::env::temp_dir().join(format!(
        "nexosign-http-contract-intent-flow-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&tmp, b"%PDF-1.4\n").unwrap();
    let abs = tmp.canonicalize().unwrap();

    let intent_body = serde_json::json!({ "inputs": [abs.to_str().unwrap()] });
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
    let bytes = to_bytes(res_intent.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let rid = v["request_id"].as_str().unwrap();

    let batch_body = serde_json::json!({
        "cert_id_hex": "01ab",
        "inputs": [abs.to_str().unwrap()],
        "job_id": "contract-intent-batch",
        "intent_request_id": rid,
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
    let _ = rx.try_recv().expect("job en cola");
    assert!(pending.lock().unwrap().is_empty());
    let _ = std::fs::remove_file(&tmp);
}
