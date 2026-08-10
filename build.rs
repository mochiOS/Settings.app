use std::env;

fn main() {
    for (name, fallback) in [
        ("MOCHIOS_VERSION", "development"),
        ("MNU_VERSION", "unavailable"),
        ("MBOOT_VERSION", "unavailable"),
        ("MOCHIOS_BUILD_NUMBER", "unavailable"),
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        if env::var_os(name).is_none() {
            println!("cargo:rustc-env={name}={fallback}");
        }
    }
}
