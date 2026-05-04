fn main() {
    let sdk_dir = std::env::var("SPOUT2_SDK_DIR").unwrap_or_default();
    let lib_dir = std::env::var("SPOUT2_LIB_DIR").unwrap_or_default();

    let header = std::path::Path::new(&sdk_dir).join("SpoutSender.h");
    let lib = std::path::Path::new(&lib_dir).join("Spout.lib");

    if !header.exists() {
        println!(
            "cargo:warning=SPOUT2_SDK_DIR not found at '{}'. Run scripts/setup_spout.ps1",
            sdk_dir
        );
    }
    if !lib.exists() {
        println!(
            "cargo:warning=SPOUT2_LIB_DIR not found at '{}'. Run scripts/setup_spout.ps1",
            lib_dir
        );
    }
}
