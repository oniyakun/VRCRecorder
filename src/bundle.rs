use std::path::PathBuf;

/// Embedded ffmpeg.exe bytes (loaded at compile time).
static EMBEDDED_FFMPEG: &[u8] = include_bytes!(env!("FFMPEG_EXE_PATH"));

/// Embedded Spout.dll bytes (loaded at compile time).
static EMBEDDED_SPOUT: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/target/release/Spout.dll"));

/// FFmpeg shared libraries that ffmpeg.exe depends on.
const FFMPEG_DLLS: &[(&str, &[u8])] = &[
    ("avcodec-61.dll", include_bytes!(env!("FFMPEG_AVCODEC"))),
    ("avdevice-61.dll", include_bytes!(env!("FFMPEG_AVDEVICE"))),
    ("avfilter-10.dll", include_bytes!(env!("FFMPEG_AVFILTER"))),
    ("avformat-61.dll", include_bytes!(env!("FFMPEG_AVFORMAT"))),
    ("avutil-59.dll", include_bytes!(env!("FFMPEG_AVUTIL"))),
    ("swresample-5.dll", include_bytes!(env!("FFMPEG_SWRESAMPLE"))),
    ("swscale-8.dll", include_bytes!(env!("FFMPEG_SWSCALE"))),
];

/// Extract bundled executables and DLLs to a runtime directory.
/// Returns the ffmpeg path. Call once at startup before any Spout/ffmpeg usage.
pub fn extract() -> Option<PathBuf> {
    let dir = dirs_extract();

    // Extract all ffmpeg DLLs
    for (name, data) in FFMPEG_DLLS {
        let path = dir.join(name);
        if !path.exists() {
            if let Err(e) = std::fs::write(&path, data) {
                log::error!("Failed to extract {name}: {e}");
            }
        }
    }

    // Extract ffmpeg.exe
    let ffmpeg_path = dir.join("ffmpeg.exe");
    if !ffmpeg_path.exists() {
        if let Err(e) = std::fs::write(&ffmpeg_path, EMBEDDED_FFMPEG) {
            log::error!("Failed to extract ffmpeg.exe: {e}");
            return None;
        }
        log::info!("Extracted ffmpeg.exe to {}", ffmpeg_path.display());
    }

    // Extract Spout.dll
    let spout_path = dir.join("Spout.dll");
    if !spout_path.exists() {
        if let Err(e) = std::fs::write(&spout_path, EMBEDDED_SPOUT) {
            log::error!("Failed to extract Spout.dll: {e}");
            return None;
        }
        log::info!("Extracted Spout.dll to {}", spout_path.display());
    }

    // Add extraction dir to DLL search path so Windows finds the ffmpeg DLLs + Spout.dll
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        unsafe extern "system" {
            fn AddDllDirectory(lpPathName: *const u16) -> *mut std::ffi::c_void;
        }
        let wide: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let _cookie = AddDllDirectory(wide.as_ptr());
    }

    log::info!("All bundled files extracted to {}", dir.display());
    Some(ffmpeg_path)
}

fn dirs_extract() -> PathBuf {
    let dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vrc-recorder");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
