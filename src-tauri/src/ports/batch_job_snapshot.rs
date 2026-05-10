//! Estado observable de un trabajo de firma por lotes (expuesto por HTTP y actualizado por el worker).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchJobPhase {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchJobSnapshot {
    pub job_id: String,
    pub phase: BatchJobPhase,
    pub actual: u32,
    pub total: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
