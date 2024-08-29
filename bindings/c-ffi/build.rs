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
}
