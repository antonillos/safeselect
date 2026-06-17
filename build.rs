use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=SAFESELECT_BUILD_VERSION");
    if let Ok(version) = env::var("SAFESELECT_BUILD_VERSION") {
        println!("cargo:rustc-env=SAFESELECT_BUILD_VERSION={version}");
    }
}
