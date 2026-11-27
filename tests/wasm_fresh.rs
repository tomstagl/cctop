//! plugin/hooks/charts.wasm is a committed build output: it must come from
//! the sources in the tree. `charts.wasm.inputs` records the SHA-256 of what
//! it was built from (same recipe as scripts/wasm-inputs.py: the cctop-charts
//! manifest and sources, and Cargo.lock without cctop's own entry); this
//! recomputes it and fails when they differ.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn sources_hash() -> String {
    let root = Path::new(ROOT);
    let crate_dir = root.join("crates/cctop-charts");
    let mut files = vec![crate_dir.join("Cargo.toml")];
    rs_files(&crate_dir.join("src"), &mut files);
    let mut named: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, p)
        })
        .collect();
    named.sort();
    let mut h = Sha256::new();
    for (rel, p) in named {
        let data = std::fs::read(p).unwrap();
        h.update(format!("{rel}\0{}\0", data.len()));
        h.update(&data);
    }
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let lock = lock
        .split("\n\n")
        .filter(|b| {
            !b.trim_start_matches('\n')
                .starts_with("[[package]]\nname = \"cctop\"\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    h.update(format!("Cargo.lock\0{}\0", lock.len()));
    h.update(lock.as_bytes());
    use std::fmt::Write as _;
    h.finalize().iter().fold(String::new(), |mut s, b| {
        write!(s, "{b:02x}").unwrap();
        s
    })
}

#[test]
fn charts_wasm_is_built_from_the_sources_in_the_tree() {
    let inputs = std::fs::read_to_string(Path::new(ROOT).join("plugin/hooks/charts.wasm.inputs"))
        .expect("plugin/hooks/charts.wasm.inputs is missing: run make wasm");
    let recorded = inputs
        .lines()
        .find_map(|l| l.strip_prefix("sources "))
        .expect("charts.wasm.inputs has no `sources` line: run make wasm");
    assert_eq!(
        recorded,
        sources_hash(),
        "plugin/hooks/charts.wasm is stale: run make wasm"
    );
}

#[test]
fn charts_wasm_is_under_600_kb() {
    let len = std::fs::metadata(Path::new(ROOT).join("plugin/hooks/charts.wasm"))
        .expect("plugin/hooks/charts.wasm is missing: run make wasm")
        .len();
    assert!(
        len < 600 * 1000,
        "charts.wasm is {len} bytes, over the 600 KB budget"
    );
}
