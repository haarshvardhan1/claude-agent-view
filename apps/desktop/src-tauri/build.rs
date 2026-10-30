use std::path::Path;

fn main() {
    ensure_sidecar_placeholder();
    tauri_build::build()
}

/// tauri-build fails when the `externalBin` sidecar is missing. The real
/// `av-hook` (the `av` CLI under another name, so tauri-build's copy into
/// `target/<profile>/` never clobbers the CLI's own `target/<profile>/av`) is
/// put there by `scripts/build-sidecar.sh` (run by `beforeDevCommand` /
/// `beforeBuildCommand`); plain `cargo build`/`cargo test` get an empty
/// placeholder so the workspace builds on a fresh clone.
fn ensure_sidecar_placeholder() {
    let triple = std::env::var("TARGET").expect("TARGET is set by cargo");
    let path = Path::new("binaries").join(format!("av-hook-{triple}"));
    println!("cargo:rerun-if-changed={}", path.display());
    if path.exists() {
        return;
    }
    std::fs::create_dir_all("binaries").expect("create binaries dir");
    std::fs::write(&path, b"").expect("write sidecar placeholder");
    println!(
        "cargo:warning=created empty sidecar placeholder {}; run scripts/build-sidecar.sh for the real `av-hook`",
        path.display()
    );
}
