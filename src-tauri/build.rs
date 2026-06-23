fn main() {
    tauri_build::build();

    // Windows: los binarios de `cargo test` no reciben el manifiesto de Tauri y fallan con
    // STATUS_ENTRYPOINT_NOT_FOUND al enlazar controles comunes v6 (tauri-apps/tauri#13419).
    #[cfg(target_os = "windows")]
    {
        let manifest_path = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
        )
        .join("common-controls.manifest");
        let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg={manifest_arg}");
        // Tauri ya embebe manifiesto en el binario principal; evitar duplicado.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    }
}
