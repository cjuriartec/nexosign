//! Comparación y normalización de orígenes para CORS (solo lógica).

/// `true` si parece una URL de origen (`scheme://…`).
pub fn is_well_formed_origin(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("null") {
        return false;
    }
    let Some((scheme, rest)) = s.split_once("://") else {
        return false;
    };
    if scheme.is_empty() || rest.is_empty() {
        return false;
    }
    scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.'))
}

/// Parsea la cabecera estándar `Origin` del navegador.
pub fn origin_from_header(origin_header: Option<&str>) -> Option<String> {
    let o = origin_header.map(str::trim).filter(|s| !s.is_empty())?;
    if is_well_formed_origin(o) {
        Some(normalize_origin(o))
    } else {
        None
    }
}

/// Origen lógico (`scheme://autoridad`) desde `Referer` cuando falta `Origin`.
pub fn origin_from_referer(referer: &str) -> Option<String> {
    let r = referer.trim();
    if r.is_empty() {
        return None;
    }
    let (scheme, rest) = r.split_once("://")?;
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|a| !a.is_empty())?;
    origin_from_header(Some(&format!("{scheme}://{authority}")))
}

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
    fn origin_from_header_accepts_any_scheme() {
        let list = vec!["https://portal.ejemplo".to_string()];
        let o = origin_from_header(Some("https://portal.ejemplo/"));
        assert_eq!(o.as_deref(), Some("https://portal.ejemplo"));
        assert!(is_origin_allowed(o.as_deref().unwrap(), &list));

        assert!(is_well_formed_origin("https://app.example"));
        assert!(is_well_formed_origin("custom+app://host"));
        assert!(!is_well_formed_origin("not-an-origin"));
        assert!(origin_from_header(None).is_none());
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

    #[test]
    fn origin_from_referer_strips_path() {
        let list = vec!["https://portal.ejemplo".to_string()];
        let o = origin_from_referer("https://portal.ejemplo/informes/listado").unwrap();
        assert_eq!(o, "https://portal.ejemplo");
        assert!(is_origin_allowed(&o, &list));
    }
}
