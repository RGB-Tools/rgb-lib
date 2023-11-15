use std::env;
use std::path::{Path, MAIN_SEPARATOR};

const DEFAULT_CLANG_VERSION: &str = "14.0.7";

/// Adds a temporary workaround for an issue with the Rust compiler and Android
/// in x86_64 devices: https://github.com/rust-lang/rust/issues/109717.
/// The workaround comes from: https://github.com/mozilla/application-services/pull/5442
fn setup_x86_64_android_workaround() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH not set");
    if target_arch == "x86_64" && target_os == "android" {
        let android_ndk_home = env::var("ANDROID_NDK_ROOT").expect("ANDROID_NDK_ROOT not set");
        let build_os = match env::consts::OS {
            "linux" => "linux",
            "macos" => "darwin",
            "windows" => "windows",
            _ => panic!(
                "Unsupported OS. You must use either Linux, MacOS or Windows to build the crate."
            ),
        };
        let clang_version =
            env::var("NDK_CLANG_VERSION").unwrap_or_else(|_| DEFAULT_CLANG_VERSION.to_owned());
        let build_os = format!("{build_os}-x86_64");
        let linux_x86_64_lib_dir = [
            "toolchains",
            "llvm",
            "prebuilt",
            &build_os,
            "lib64",
            "clang",
            &clang_version,
            "lib",
            "linux",
            "",
        ]
        .join(&MAIN_SEPARATOR.to_string());
        let linkpath = [android_ndk_home.clone(), linux_x86_64_lib_dir.clone()]
            .join(&MAIN_SEPARATOR.to_string());
        if Path::new(&linkpath).exists() {
            println!("cargo:rustc-link-search={android_ndk_home}/{linux_x86_64_lib_dir}");
            println!("cargo:rustc-link-lib=static=clang_rt.builtins-x86_64-android");
        } else {
            panic!("Path {linkpath} not exists");
        }
    }
}

fn main() {
    let udl_file = ["src", "rgb-lib.udl"].join(&MAIN_SEPARATOR.to_string());
    setup_x86_64_android_workaround();
    uniffi::generate_scaffolding(udl_file).expect("UDL should be valid");
}
