//! tiny-skia's `simd` feature must stay off anywhere in the workspace: with
//! it, x86 uses approximate reciprocals (`_mm_rcp_ps`) and the pictures, and
//! their hashes in `render.rs`, would differ between machines and wasm.

use std::process::Command;

#[test]
fn tiny_skia_is_built_without_simd() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let out = Command::new(cargo)
        .args(["tree", "--workspace", "-e", "features", "--prefix", "none"])
        .args(["--manifest-path", manifest])
        .output()
        .expect("cargo tree runs");
    assert!(
        out.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let tree = String::from_utf8_lossy(&out.stdout);
    let features: Vec<&str> = tree
        .lines()
        .filter(|l| l.starts_with("tiny-skia feature \""))
        .collect();
    // The parse works: the one feature asked for is there.
    assert!(
        features
            .iter()
            .any(|l| l.starts_with("tiny-skia feature \"std\"")),
        "no tiny-skia features found in:\n{tree}"
    );
    assert!(
        !features
            .iter()
            .any(|l| l.starts_with("tiny-skia feature \"simd\"")
                || l.starts_with("tiny-skia feature \"default\"")),
        "tiny-skia's simd (or default) feature is on: {features:?}"
    );
}
