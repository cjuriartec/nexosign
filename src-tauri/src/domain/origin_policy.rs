//! Comparación y normalización de orígenes para CORS (solo lógica).

/// Normaliza para comparación estable (trim, sin barra final salvo raíz).
///
/// Corrige el error típico `http://127.0.0.1.:14500` (punto de más antes del puerto).
pub fn normalize_origin(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches('/').to_string();
    if s.contains("127.0.0.1.:") {
        s = s.replace("127.0.0.1.:", "127.0.0.1:");
    }
    s
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

    #[test]
    fn normalize_fixes_extra_dot_before_port() {
        assert_eq!(
            normalize_origin("http://127.0.0.1.:14500"),
            "http://127.0.0.1:14500"
        );
        let list = vec![normalize_origin("http://127.0.0.1.:14500")];
        assert!(is_origin_allowed("http://127.0.0.1:14500", &list));
    }
}
