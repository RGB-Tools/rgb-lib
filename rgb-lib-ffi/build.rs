fn main() {
    uniffi::generate_scaffolding("src/rgb-lib.udl").expect("UDL should be valid");
}
