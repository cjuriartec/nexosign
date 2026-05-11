//! Lógica compartida entre Axum (integradores HTTP) y comandos Tauri (`invoke` de la UI).
//! Mantiene una sola implementación de encolado y lectura de estado batch.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use base64::Engine;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::{validate_optional_output_dir, BatchSignBody, BatchSignResponse};
use crate::adapters::http::state::{HealthResponse, PingResponse, SharedState};
use crate::adapters::persistence::queue_store;
use crate::adapters::worker::batch::BatchJob;
use crate::infrastructure::batch_pdf_validation::validate_batch_pdf_inputs;
use crate::ports::{BatchJobPhase, BatchJobSnapshot, SignatureGridPlacement};

/// Error de operación local mapeable a HTTP y a [`LocalApiInvokeError`].
#[derive(Debug)]
pub struct LocalApiOpError {
    pub status: StatusCode,
    pub code: String,
    pub detail: String,
}

impl LocalApiOpError {
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request".into(),
            detail: detail.into(),
        }
    }

    pub fn not_found(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: code.into(),
            detail: detail.into(),
        }
    }

    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "service_unavailable".into(),
            detail: detail.into(),
        }
    }

    pub fn internal(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: code.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiInvokeError {
    pub code: String,
    pub detail: String,
}

impl From<LocalApiOpError> for LocalApiInvokeError {
    fn from(e: LocalApiOpError) -> Self {
        LocalApiInvokeError {
            code: e.code,
            detail: e.detail,
        }
    }
}

pub fn health_payload() -> HealthResponse {
    HealthResponse {
        status: "ok",
        service: "nexosign",
        version: env!("CARGO_PKG_VERSION"),
    }
}

pub fn ping_payload() -> PingResponse {
    PingResponse { ok: true }
}

/// Encola firma por lotes (misma semántica que `POST /api/v1/batch/sign` sin comprobación CORS).
pub fn try_enqueue_batch_sign(
    state: &SharedState,
    body: BatchSignBody,
) -> Result<BatchSignResponse, LocalApiOpError> {
    let Some(tx) = state.batch_tx.as_ref() else {
        return Err(LocalApiOpError::service_unavailable("cola batch no configurada"));
    };

    if body.cert_id_hex.trim().is_empty() {
        return Err(LocalApiOpError::bad_request("cert_id_hex requerido"));
    }

    if let Err(msg) = validate_batch_pdf_inputs(&body.inputs) {
        return Err(LocalApiOpError::bad_request(msg));
    }

    let output_dir = validate_optional_output_dir(body.output_dir).map_err(LocalApiOpError::bad_request)?;

    let signature_grid = match body.signature_grid {
        Some(g) => {
            if g.col > 2 || g.row > 4 {
                return Err(LocalApiOpError::bad_request(
                    "signature_grid: col debe ser 0–2 y row 0–4 (rejilla 3×5)",
                ));
            }
            Some(SignatureGridPlacement { col: g.col, row: g.row })
        }
        None => None,
    };

    if let Some(ref pin_raw) = body.pin {
        if pin_raw.trim().is_empty() {
            return Err(LocalApiOpError::bad_request("PIN vacío"));
        }
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
                        return Err(LocalApiOpError::bad_request(
                            "signature_seal_png_base64 supera 1,5 MiB",
                        ));
                    }
                    Err(_) => {
                        return Err(LocalApiOpError::bad_request(
                            "signature_seal_png_base64 no es base64 válido",
                        ));
                    }
                }
            }
        }
    };

    let job_id = body
        .job_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let input_count = body.inputs.len();
    let cancel = CancellationToken::new();
    {
        let mut g = state
            .batch_cancel
            .lock()
            .map_err(|_| LocalApiOpError::internal("batch_snapshots_locked", "estado batch bloqueado"))?;
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
                if let Some(ref db_arc) = state.queue_sqlite_path {
                    let _ = queue_store::delete_intent_payload(db_arc.as_ref(), &rid);
                }
                if let Ok(mut m) = state.intent_request_to_job.lock() {
                    m.insert(rid, job_id.clone());
                }
            }
            if let Ok(mut m) = state.batch_job_snapshots.lock() {
                let queued_at_unix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .ok();
                m.insert(
                    job_id.clone(),
                    BatchJobSnapshot {
                        job_id: job_id.clone(),
                        phase: BatchJobPhase::Queued,
                        actual: 0,
                        total: u32::try_from(input_count).unwrap_or(1).max(1),
                        queued_at_unix,
                        current_file_name: None,
                        error: None,
                        terminal_at_unix: None,
                    },
                );
                if let (Some(ref db_arc), Some(ts)) = (&state.queue_sqlite_path, queued_at_unix) {
                    let _ = queue_store::upsert_batch_job_enqueue(db_arc.as_ref(), &job_id, ts);
                }
            }
            Ok(BatchSignResponse {
                job_id,
                queued: true,
            })
        }
        Err(tokio::sync::mpsc::error::TrySendError::Full(j)) => {
            if let Ok(mut g) = state.batch_cancel.lock() {
                g.remove(&j.job_id);
            }
            Err(LocalApiOpError::service_unavailable(
                "cola batch llena, reintente",
            ))
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(j)) => {
            if let Ok(mut g) = state.batch_cancel.lock() {
                g.remove(&j.job_id);
            }
            Err(LocalApiOpError::service_unavailable("cola batch cerrada"))
        }
    }
}

/// Lee snapshot de trabajo (misma semántica que `GET /api/v1/batch/jobs/{job_id}/status` sin CORS).
pub fn try_get_batch_job_snapshot(
    state: &SharedState,
    job_id: &str,
) -> Result<BatchJobSnapshot, LocalApiOpError> {
    let guard = state
        .batch_job_snapshots
        .lock()
        .map_err(|_| LocalApiOpError::internal("batch_snapshots_locked", "estado batch snapshots bloqueado"))?;
    guard
        .get(job_id)
        .cloned()
        .ok_or_else(|| {
            LocalApiOpError::not_found(
                "job_not_found",
                "Sin estado para este job_id.",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::state::SharedState;

    #[test]
    fn job_status_unknown_returns_not_found() {
        let state = SharedState::test_default();
        let err = try_get_batch_job_snapshot(&state, "no-existe").unwrap_err();
        assert_eq!(err.code, "job_not_found");
        assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn enqueue_without_queue_returns_503() {
        let state = SharedState::test_default();
        let body = BatchSignBody {
            cert_id_hex: "01ab".into(),
            inputs: vec![],
            job_id: None,
            pin: None,
            output_dir: None,
            signature_grid: None,
            intent_request_id: None,
            signature_seal_png_base64: None,
        };
        let err = try_enqueue_batch_sign(&state, body).unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
