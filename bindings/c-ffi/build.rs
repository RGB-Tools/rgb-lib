extern crate cbindgen;

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=NULL");

    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    cbindgen::generate(&crate_dir)
        .expect("Unable to generate C bindings")
        .write_to_file("rgblib.h");

    let mut config = cbindgen::Config::from_file("cbindgen.toml").unwrap();
    config.language = cbindgen::Language::Cxx;
    cbindgen::generate_with_config(&crate_dir, config)
        .expect("Unable to generate C++ bindings")
        .write_to_file("rgblib.hpp");

    let target = std::env::var("TARGET").ok();
    if let Some(t) = target {
        if t.contains("apple-darwin") {
            println!("cargo:rustc-link-arg=-Wl,-install_name,@rpath/librgblibcffi.dylib");
        }
    } else {
        println!("cargo:warning=TARGET environment variable not set");
    }
}
