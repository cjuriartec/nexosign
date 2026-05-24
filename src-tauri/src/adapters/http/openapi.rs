//! Documentación OpenAPI servida en **`GET /openapi.json`** y Swagger UI en **`GET /docs`**.
//!
//! El JSON describe el contrato pensado para **integradores** (intent, estado del intent, descargas).
//! Rutas usadas solo por la app de escritorio en la misma máquina **no** están en este archivo.
//!
//! La URL del servidor en el spec se sustituye por el puerto efectivo en runtime.

use axum::extract::State;
use axum::http::header;
use axum::response::{Html, IntoResponse};

use super::state::SharedState;
use crate::infrastructure::local_api_listen::{
    base_url_for_port, LOCAL_API_DEFAULT_PORT,
};

pub const OPENAPI_JSON_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/openapi/nexosign-local-api.openapi.json"
));

pub const SWAGGER_UI_HTML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/openapi/swagger-ui.html"
));

fn openapi_json_for_base_url(base_url: &str) -> String {
    let default_base = base_url_for_port(LOCAL_API_DEFAULT_PORT);
    OPENAPI_JSON_TEMPLATE.replace(&default_base, base_url)
}

pub async fn get_openapi_json(State(state): State<SharedState>) -> impl IntoResponse {
    let base_url = base_url_for_port(state.local_api.gate_listen_port());
    let body = openapi_json_for_base_url(&base_url);
    ([(header::CONTENT_TYPE, "application/json")], body)
}

pub async fn get_api_docs() -> Html<&'static str> {
    Html(SWAGGER_UI_HTML)
}
