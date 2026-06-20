fn main() {
    // --- Generate icons BEFORE Tauri build (needs icons/icon.ico) ---
    let png_path = std::path::Path::new("logo.png");
    if png_path.exists() {
        let png_data = std::fs::read(png_path).expect("Failed to read logo.png");
        let img = image::load_from_memory(&png_data).expect("Failed to decode logo.png");
        let img = img.resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
        let rgba = img.to_rgba8();
        let ico_bytes = build_ico(&rgba);

        // Write to icons/ for Tauri
        let icons_dir = std::path::Path::new("icons");
        let _ = std::fs::create_dir_all(icons_dir);
        std::fs::write(icons_dir.join("icon.ico"), &ico_bytes)
            .expect("Failed to write icons/icon.ico");
    }

    // --- Tauri build (expects icons/icon.ico to exist) ---
    tauri_build::build();
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

    // --- Copy Spout.dll to target ---
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let spout_src = std::path::PathBuf::from("vendor/Spout2/cmake-build/bin/Release/Spout.dll");
    let spout_dest = std::path::PathBuf::from("target")
        .join(&profile)
        .join("Spout.dll");
    if spout_src.exists() {
        let _ = std::fs::create_dir_all(spout_dest.parent().unwrap());
        let _ = std::fs::copy(&spout_src, &spout_dest);
    }
    // Set env var for bundle.rs to find the Spout.dll (absolute path needed for include_bytes!)
    let spout_abs = spout_dest
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap().join(&spout_dest));
    println!("cargo:rustc-env=SPOUT_DLL_PATH={}", spout_abs.display());
    // Also set ffmpeg path and all DLL paths
    let ffmpeg_path =
        std::path::PathBuf::from("C:/Program Files/Virtual Desktop Streamer/ffmpeg.exe");
    if ffmpeg_path.exists() {
        println!("cargo:rustc-env=FFMPEG_EXE_PATH={}", ffmpeg_path.display());
        let ffmpeg_dir = ffmpeg_path.parent().unwrap();
        let d = ffmpeg_dir.display();
        // FFmpeg shared libraries that ffmpeg.exe depends on
        println!("cargo:rustc-env=FFMPEG_AVCODEC={d}/avcodec-61.dll");
        println!("cargo:rustc-env=FFMPEG_AVDEVICE={d}/avdevice-61.dll");
        println!("cargo:rustc-env=FFMPEG_AVFILTER={d}/avfilter-10.dll");
        println!("cargo:rustc-env=FFMPEG_AVFORMAT={d}/avformat-61.dll");
        println!("cargo:rustc-env=FFMPEG_AVUTIL={d}/avutil-59.dll");
        println!("cargo:rustc-env=FFMPEG_SWRESAMPLE={d}/swresample-5.dll");
        println!("cargo:rustc-env=FFMPEG_SWSCALE={d}/swscale-8.dll");
    } else {
        println!("cargo:warning=ffmpeg.exe not found at Virtual Desktop Streamer path");
    }

    // Copy logo.png to frontend directory for the sidebar
    let logo_src = std::path::Path::new("logo.png");
    let logo_dst = std::path::Path::new("frontend/logo.png");
    if logo_src.exists() {
        let _ = std::fs::copy(logo_src, logo_dst);
    }

    // Copy sound files to frontend directory
    let sound_src = std::path::Path::new("sound");
    let sound_dst = std::path::Path::new("frontend/sound");
    if sound_src.exists() {
        let _ = std::fs::create_dir_all(sound_dst);
        for entry in
            std::fs::read_dir(sound_src).unwrap_or_else(|_| std::fs::read_dir(".").unwrap())
        {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "mp3") {
                    let dst = sound_dst.join(path.file_name().unwrap());
                    let _ = std::fs::copy(&path, &dst);
                }
            }
        }
    }

    // --- Delay-load Spout.dll (extracted from embedded bytes at runtime) ---
    println!("cargo:rustc-link-arg=/DELAYLOAD:Spout.dll");
    println!("cargo:rustc-link-arg=DelayImp.lib");
}

/// Build a minimal .ico file containing a single 256×256 PNG entry.
fn build_ico(rgba: &image::RgbaImage) -> Vec<u8> {
    use image::ImageEncoder;
    use std::io::Write;
    let (w, h) = rgba.dimensions();

    // Re-encode as PNG for the ICO data block
    let mut png_buf = Vec::new();
    {
        let mut encoder = image::codecs::png::PngEncoder::new(&mut png_buf);
        encoder
            .write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .expect("PNG encode failed");
    }

    let data_size = png_buf.len() as u32;
    let data_offset: u32 = 6 + 16; // header + 1 entry

    let mut ico = Vec::new();
    // ICO header
    ico.write_all(&[0u8, 0]).unwrap(); // reserved
    ico.write_all(&[1u8, 0]).unwrap(); // type: ICO = 1
    ico.write_all(&[1u8, 0]).unwrap(); // count: 1 image

    // Directory entry
    let w8 = if w >= 256 { 0u8 } else { w as u8 };
    let h8 = if h >= 256 { 0u8 } else { h as u8 };
    ico.write_all(&[w8]).unwrap();
    ico.write_all(&[h8]).unwrap();
    ico.write_all(&[0u8]).unwrap(); // palette size
    ico.write_all(&[0u8]).unwrap(); // reserved
    ico.write_all(&[1u8, 0]).unwrap(); // planes
    ico.write_all(&[32u8, 0]).unwrap(); // bpp
    ico.write_all(&data_size.to_le_bytes()).unwrap(); // size of image data
    ico.write_all(&data_offset.to_le_bytes()).unwrap(); // offset to image data

    // PNG data
    ico.extend_from_slice(&png_buf);

    ico
}
