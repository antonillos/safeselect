use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=SAFESELECT_BUILD_VERSION");
    if let Ok(version) = env::var("SAFESELECT_BUILD_VERSION") {
        println!("cargo:rustc-env=SAFESELECT_BUILD_VERSION={version}");
    }
    println!("cargo:rerun-if-changed=sidecar/target/safeselect-sidecar.jar");
    println!("cargo:warning=Building with JAR: sidecar/target/safeselect-sidecar.jar");
}
