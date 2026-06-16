use std::path::Path;

fn main() {
    let sidecar_dir = Path::new("sidecar/target");
    let versioned = sidecar_dir.join("safeselect-sidecar-0.1.0.jar");
    let expected = sidecar_dir.join("safeselect-sidecar.jar");

    if versioned.exists() {
        std::fs::copy(&versioned, &expected).ok();
    }

    if expected.exists() {
        println!("cargo:rerun-if-changed={}", expected.display());
    }
    println!("cargo:rerun-if-changed=sidecar/src");
}
