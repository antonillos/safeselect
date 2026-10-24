use std::env;

fn main() {
    for name in [
        "SAFESELECT_BUILD_VERSION",
        "SAFESELECT_SOURCE_DIR",
        "SAFESELECT_SOURCE_MODE",
        "SAFESELECT_SOURCE_BRANCH",
    ] {
        forward_build_setting(name);
    }
    println!("cargo:rerun-if-changed=sidecar/target/safeselect-sidecar.jar");
    println!("cargo:warning=Building with JAR: sidecar/target/safeselect-sidecar.jar");
}

fn forward_build_setting(name: &str) {
    println!("cargo:rerun-if-env-changed={name}");
    if let Ok(value) = env::var(name) {
        println!("cargo:rustc-env={name}={value}");
    }
}
