//! Notificación de progreso del lote (sin acoplar a Tauri).

#[derive(Clone, Debug)]
pub struct ProgressEvent {
    pub job_id: String,
    pub current: u32,
    pub total: u32,
    pub file_name: String,
    pub path: String,
    pub error: Option<String>,
}

pub trait ProgressNotifier: Send + Sync {
    fn notify(&self, ev: ProgressEvent);
}

impl ProgressNotifier for Box<dyn ProgressNotifier> {
    fn notify(&self, ev: ProgressEvent) {
        (**self).notify(ev);
    }
}

#[derive(Clone, Default)]
pub struct NoopProgressNotifier;

impl ProgressNotifier for NoopProgressNotifier {
    fn notify(&self, _ev: ProgressEvent) {}
}
