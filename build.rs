//! Gives the Windows executables their icon (`assets/icon.ico`); nothing elsewhere.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    embed_resource::compile("assets/icon.rc", embed_resource::NONE).manifest_optional().unwrap();
}
