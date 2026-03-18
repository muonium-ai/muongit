use std::fs;

fn main() {
    let version = fs::read_to_string("../VERSION")
        .expect("Failed to read VERSION file")
        .trim()
        .to_string();
    println!("cargo:rustc-env=MUONGIT_VERSION={}", version);
    println!("cargo:rerun-if-changed=../VERSION");
}
