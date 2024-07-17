use std::path::MAIN_SEPARATOR_STR;

fn main() {
    let udl_file = ["src", "rgb-lib.udl"].join(MAIN_SEPARATOR_STR);
    uniffi::generate_scaffolding(udl_file).expect("UDL should be valid");
}
