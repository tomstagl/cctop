#!/usr/bin/env python3
"""SHA-256 of what charts.wasm is built from: the cctop-charts manifest and
sources, and Cargo.lock without cctop's own entry (its version moves on
every release and does not reach the wasm). Prints the hex digest.
tests/wasm_fresh.rs recomputes the same thing; keep the two in step."""
import hashlib, pathlib

root = pathlib.Path(__file__).resolve().parent.parent
crate = root / "crates/cctop-charts"
files = [crate / "Cargo.toml"] + sorted(crate.glob("src/**/*.rs"))
h = hashlib.sha256()
for f in sorted(files, key=lambda p: p.relative_to(root).as_posix()):
    data = f.read_bytes()
    h.update(f"{f.relative_to(root).as_posix()}\0{len(data)}\0".encode())
    h.update(data)
blocks = (root / "Cargo.lock").read_text().split("\n\n")
lock = "\n\n".join(
    b for b in blocks if not b.lstrip("\n").startswith('[[package]]\nname = "cctop"\n')
).encode()
h.update(f"Cargo.lock\0{len(lock)}\0".encode())
h.update(lock)
print(h.hexdigest())
