use std::path::MAIN_SEPARATOR;


fn main() {
    let udl_file = ["src", "rgb-lib.udl"].join(&MAIN_SEPARATOR.to_string());
    uniffi::generate_scaffolding(udl_file).expect("UDL should be valid");
}
