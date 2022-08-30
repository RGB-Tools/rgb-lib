fn main() {
    uniffi_build::generate_scaffolding("src/rgb-lib.udl").expect("UDL should be valid");
}
