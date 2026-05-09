//! Comparación y normalización de orígenes para CORS (solo lógica).

/// Normaliza para comparación estable (trim, sin barra final salvo raíz).
pub fn normalize_origin(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

/// Comprueba si `origin` está en la lista `allowed` (tras normalizar ambos lados).
pub fn is_origin_allowed(origin: &str, allowed: &[String]) -> bool {
    let n = normalize_origin(origin);
    allowed.iter().any(|a| normalize_origin(a) == n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_strips_trailing_slash() {
        assert_eq!(
            normalize_origin("  http://localhost:1420/  "),
            "http://localhost:1420"
        );
    }

    #[test]
    fn matches_ignore_trailing_slash_mismatch() {
        let list = vec!["http://localhost:1420".to_string()];
        assert!(is_origin_allowed("http://localhost:1420/", &list));
    }

    #[test]
    fn rejects_unknown_origin() {
        let list = vec!["http://localhost:1420".to_string()];
        assert!(!is_origin_allowed("https://evil.test", &list));
    }
}
