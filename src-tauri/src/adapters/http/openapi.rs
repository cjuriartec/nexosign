//! Documentación OpenAPI servida en **`GET /openapi.json`** y Swagger UI en **`GET /docs`**.
//!
//! El JSON describe el contrato pensado para **integradores** (intent, estado del intent, descargas).
//! Rutas usadas solo por la app de escritorio en la misma máquina **no** están en este archivo.
//!
//! También puedes copiar la URL `http://127.0.0.1:14500/openapi.json` en [Scalar](https://scalar.com),
//! Stoplight, Postman, etc., con la aplicación en ejecución.

use axum::http::header;
use axum::response::{Html, IntoResponse};

pub const OPENAPI_JSON: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/openapi/nexosign-local-api.openapi.json"
));

pub const SWAGGER_UI_HTML: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/openapi/swagger-ui.html"
));

pub async fn get_openapi_json() -> impl IntoResponse {
	([(header::CONTENT_TYPE, "application/json")], OPENAPI_JSON)
}

pub async fn get_api_docs() -> Html<&'static str> {
	Html(SWAGGER_UI_HTML)
}
