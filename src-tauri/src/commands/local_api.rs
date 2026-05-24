//! Comandos Tauri para la misma funcionalidad que expone la API HTTP en loopback (UI sin `fetch`).

use serde::Serialize;
use tauri::State;

use crate::adapters::http::local_api_ops::{self, LocalApiInvokeError};
use crate::adapters::http::state::{HealthResponse, PingResponse, SharedState};
use crate::adapters::http::{BatchSignBody, BatchSignResponse};
use crate::ports::{BatchJobPhase, BatchJobSnapshot};

/// Respuestas IPC en `camelCase` alineadas con el resto de comandos Tauri ([`crate::commands::BatchSignIntentPayload`]).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSignIpcResponse {
    pub job_id: String,
    pub queued: bool,
}

impl From<BatchSignResponse> for BatchSignIpcResponse {
    fn from(r: BatchSignResponse) -> Self {
        Self {
            job_id: r.job_id,
            queued: r.queued,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchJobStatusIpcResponse {
    pub job_id: String,
    pub phase: BatchJobPhase,
    pub actual: u32,
    pub total: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_at_unix: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_at_unix: Option<i64>,
}

impl From<BatchJobSnapshot> for BatchJobStatusIpcResponse {
    fn from(s: BatchJobSnapshot) -> Self {
        Self {
            job_id: s.job_id,
            phase: s.phase,
            actual: s.actual,
            total: s.total,
            queued_at_unix: s.queued_at_unix,
            current_file_name: s.current_file_name,
            error: s.error,
            terminal_at_unix: s.terminal_at_unix,
        }
    }
}

#[tauri::command]
pub fn local_api_health(state: State<'_, SharedState>) -> HealthResponse {
    local_api_ops::health_payload(state.inner())
}

#[tauri::command]
pub fn local_api_ping() -> PingResponse {
    local_api_ops::ping_payload()
}

#[tauri::command]
pub fn local_api_enqueue_batch_sign(
    state: State<'_, SharedState>,
    body: BatchSignBody,
) -> Result<BatchSignIpcResponse, LocalApiInvokeError> {
    local_api_ops::try_enqueue_batch_sign(state.inner(), body)
        .map(Into::into)
        .map_err(Into::into)
}

#[tauri::command]
pub fn local_api_batch_job_status(
    state: State<'_, SharedState>,
    job_id: String,
) -> Result<BatchJobStatusIpcResponse, LocalApiInvokeError> {
    local_api_ops::try_get_batch_job_snapshot(state.inner(), &job_id)
        .map(Into::into)
        .map_err(Into::into)
}
