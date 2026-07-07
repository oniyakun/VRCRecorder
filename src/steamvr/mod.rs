//! SteamVR dashboard overlay integration.
//!
//! Follows the OpenVR-AdvancedSettings pattern:
//! 1. Init VR as VRApplication_Overlay
//! 2. Validate interface versions
//! 3. Register application manifest (IVRApplications)
//! 4. Set SteamVR Input action manifest (IVRInput)
//! 5. Create dashboard overlay (IVROverlay)
//! 6. Main loop: poll events + input actions + update texture

mod ffi;
mod texture;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crate::channels::RenderCommand;
use crate::config::AppConfig;

use ffi::{
    IVRAPPLICATIONS_VERSION, IVRINPUT_VERSION, IVROVERLAY_VERSION, InputDigitalActionData,
    VR_APPLICATION_OVERLAY, VRActiveActionSet, VREVENT_DASHBOARD_ACTIVATED, VREVENT_QUIT, VREvent,
    get_interface, launch_steamvr, load_openvr,
};

const APP_KEY: &str = "com.vrc-recorder.overlay";
const ACTION_MANIFEST_FILE: &str = "steamvr_action_manifest.json";

/// Commands sent from the frontend to the SteamVR overlay thread.
#[derive(Debug)]
pub enum OverlayCommand {
    /// Open the SteamVR controller binding UI on desktop.
    OpenBindingUI,
}

// Overlay input method: Mouse
const VROVERLAY_INPUT_METHOD_MOUSE: i32 = 2;
// Flag for smooth scrolling
const VROVERLAY_FLAGS_SEND_VR_SMOOTH_SCROLL_EVENTS: i32 = 3;

/// Spawn the SteamVR overlay thread. Returns immediately if config disables it.
pub fn spawn_overlay(
    config: AppConfig,
    cmd_tx: mpsc::Sender<RenderCommand>,
    is_recording: Arc<AtomicBool>,
    auto_launch: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    language: Arc<Mutex<Option<String>>>,
    overlay_cmd_rx: mpsc::Receiver<OverlayCommand>,
) {
    enabled.store(config.steamvr_overlay, Ordering::Relaxed);

    std::thread::Builder::new()
        .name("steamvr-overlay".into())
        .spawn(move || {
            loop {
                if !enabled.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }

                overlay_main(
                    cmd_tx.clone(),
                    is_recording.clone(),
                    auto_launch.clone(),
                    enabled.clone(),
                    language.clone(),
                    &overlay_cmd_rx,
                );
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        })
        .expect("Failed to spawn SteamVR overlay thread");
}

fn overlay_main(
    cmd_tx: mpsc::Sender<RenderCommand>,
    is_recording: Arc<AtomicBool>,
    auto_launch: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    language: Arc<Mutex<Option<String>>>,
    overlay_cmd_rx: &mpsc::Receiver<OverlayCommand>,
) {
    // ---- Step 1: Load DLL (retry until SteamVR is available) ----
    let dll = loop {
        if !enabled.load(Ordering::Relaxed) {
            log::info!("SteamVR overlay disabled before DLL load");
            return;
        }
        match load_openvr() {
            Ok(dll) => break dll,
            Err(e) => {
                log::info!("SteamVR overlay: {e} — retrying in 5s");
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    };
    log::info!("SteamVR overlay: DLL loaded");

    // ---- Step 2: Init OpenVR as overlay application ----
    // OpenVR-AdvancedSettings uses VRApplication_Overlay with CreateDashboardOverlay successfully.
    // While VR_InitInternal keeps failing, SteamVR isn't running yet; if the user enabled
    // auto-launch, start it once via vrstartup.exe and keep retrying until it comes up.
    let mut launch_attempted = false;
    loop {
        if !enabled.load(Ordering::Relaxed) {
            log::info!("SteamVR overlay disabled before VR init");
            return;
        }
        // Small delay to let SteamVR fully initialize (OyasumiVR does this)
        std::thread::sleep(std::time::Duration::from_secs(1));
        unsafe {
            let mut error: i32 = 0;
            let vr = (dll.vr_init)(&mut error, VR_APPLICATION_OVERLAY);
            if vr.is_null() {
                // Read the flag live so toggling it in the UI takes effect while we wait.
                if !launch_attempted && auto_launch.load(Ordering::Relaxed) {
                    launch_attempted = true;
                    match launch_steamvr() {
                        Ok(()) => log::info!("SteamVR overlay: auto-launching SteamVR (vrstartup.exe)"),
                        Err(e) => log::warn!("SteamVR overlay: auto-launch failed: {e}"),
                    }
                }
                log::info!(
                    "SteamVR overlay: VR_InitInternal failed (error={error}) — retrying in 3s"
                );
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }
        }
        break;
    }
    log::info!("SteamVR overlay: VR initialized as Overlay application");

    // ---- Step 3: Get interfaces ----
    log::info!("SteamVR overlay: getting VROverlay...");
    let overlay = match get_interface(&dll, IVROVERLAY_VERSION) {
        Ok(ptr) => {
            log::info!("SteamVR overlay: VROverlay ptr={ptr:p}");
            unsafe { ffi::VROverlay::new(ptr) }
        }
        Err(e) => {
            log::error!("SteamVR overlay: get IVROverlay failed: {e}");
            unsafe {
                (dll.vr_shutdown)();
            }
            return;
        }
    };

    log::info!("SteamVR overlay: getting IVRApplications...");
    let applications = match get_interface(&dll, IVRAPPLICATIONS_VERSION) {
        Ok(ptr) => {
            log::info!("SteamVR overlay: IVRApplications ptr={ptr:p}");
            unsafe { ffi::VRApplications::new(ptr) }
        }
        Err(e) => {
            log::error!("SteamVR overlay: get IVRApplications failed: {e}");
            unsafe {
                (dll.vr_shutdown)();
            }
            return;
        }
    };

    log::info!("SteamVR overlay: getting IVRInput...");
    let input = match get_interface(&dll, IVRINPUT_VERSION) {
        Ok(ptr) => {
            log::info!("SteamVR overlay: IVRInput ptr={ptr:p}");
            unsafe { ffi::VRInput::new(ptr) }
        }
        Err(e) => {
            log::error!("SteamVR overlay: get IVRInput failed: {e}");
            unsafe {
                (dll.vr_shutdown)();
            }
            return;
        }
    };

    log::info!("SteamVR overlay: interfaces obtained OK");

    // ---- Step 4: Write manifests to persistent data dir ----
    log::info!("SteamVR overlay: step 4 - writing manifests...");
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vrc-recorder");
    let _ = std::fs::create_dir_all(&data_dir);

    let manifest_path = data_dir.join("manifest.vrmanifest");
    let action_manifest_path = data_dir.join(ACTION_MANIFEST_FILE);

    if let Err(e) = write_manifest(&manifest_path, &action_manifest_path) {
        log::error!("SteamVR overlay: failed to write manifest.vrmanifest: {e}");
    }

    if let Err(e) = std::fs::write(&action_manifest_path, include_str!("action_manifest.json")) {
        log::error!("SteamVR overlay: failed to write action_manifest.json: {e}");
    }
    for (name, contents) in [
        (
            "bindings_knuckles.json",
            include_str!("bindings_knuckles.json"),
        ),
        (
            "bindings_oculus_touch.json",
            include_str!("bindings_oculus_touch.json"),
        ),
        (
            "bindings_vive_controller.json",
            include_str!("bindings_vive_controller.json"),
        ),
        (
            "bindings_pico4.json",
            include_str!("bindings_pico4.json"),
        ),
    ] {
        if let Err(e) = std::fs::write(data_dir.join(name), contents) {
            log::error!("SteamVR overlay: failed to write {name}: {e}");
        }
    }
    let tex_path = data_dir.join("steamvr_overlay.png");
    log::info!(
        "SteamVR overlay: step 4 done, action manifest at {}",
        action_manifest_path.display()
    );

    // ---- Step 5: Register application manifest ----
    log::info!("SteamVR overlay: step 5 - registering application manifest...");
    register_application_manifest(&applications, &manifest_path);
    log::info!("SteamVR overlay: step 5 done");

    // ---- Step 6: Set SteamVR Input action manifest ----
    log::info!("SteamVR overlay: step 6 - setting action manifest...");
    let action_manifest_cstr =
        std::ffi::CString::new(action_manifest_path.to_string_lossy().as_bytes()).unwrap();
    unsafe {
        let err = input.set_action_manifest_path(action_manifest_cstr.as_ptr() as *const u8);
        if err != 0 {
            log::warn!("SteamVR overlay: SetActionManifestPath error={err}");
        } else {
            log::info!("SteamVR overlay: action manifest set");
        }
    }
    log::info!("SteamVR overlay: step 6 done");

    // ---- Step 7: Get action handles ----
    log::info!("SteamVR overlay: step 7 - getting action handles...");
    let action_set_cstr = std::ffi::CString::new("/actions/vrc_recorder").unwrap();
    let mut action_set_handle: u64 = 0;
    unsafe {
        log::info!("SteamVR overlay: step 7a - GetActionSetHandle...");
        let err = input.get_action_set_handle(
            action_set_cstr.as_ptr() as *const u8,
            &mut action_set_handle,
        );
        if err != 0 {
            log::error!("SteamVR overlay: GetActionSetHandle failed (error={err})");
            unsafe {
                (dll.vr_shutdown)();
            }
            return;
        }
        log::info!("SteamVR overlay: action set handle={action_set_handle}");
    }

    let action_cstr = std::ffi::CString::new("/actions/vrc_recorder/in/toggle_record").unwrap();
    let mut toggle_action: u64 = 0;
    unsafe {
        log::info!("SteamVR overlay: step 7b - GetActionHandle...");
        let err = input.get_action_handle(action_cstr.as_ptr() as *const u8, &mut toggle_action);
        if err != 0 {
            log::error!("SteamVR overlay: GetActionHandle failed (error={err})");
            unsafe {
                (dll.vr_shutdown)();
            }
            return;
        }
        log::info!("SteamVR overlay: action handle={toggle_action}");
    }
    log::info!("SteamVR overlay: step 7 done");

    // ---- Step 8: Create dashboard overlay ----
    log::info!("SteamVR overlay: step 8 - creating dashboard overlay...");
    let overlay_key = std::ffi::CString::new(APP_KEY).unwrap();
    let overlay_name = std::ffi::CString::new("VRCRecorder").unwrap();
    let mut main_handle: u64 = 0;
    let mut thumb_handle: u64 = 0;

    unsafe {
        log::info!("SteamVR overlay: step 8a - CreateDashboardOverlay...");
        let err = overlay.create_dashboard_overlay(
            overlay_key.as_ptr() as *const u8,
            overlay_name.as_ptr() as *const u8,
            &mut main_handle,
            &mut thumb_handle,
        );
        log::info!(
            "SteamVR overlay: CreateDashboardOverlay returned err={err}, main={main_handle}, thumb={thumb_handle}"
        );

        // Configure overlay (following AdvancedSettings pattern)
        if main_handle != 0 {
            log::info!("SteamVR overlay: step 8b - configuring overlay (width, input, flags)...");
            overlay.set_overlay_width_in_meters(main_handle, 2.5);
            overlay.set_overlay_input_method(main_handle, VROVERLAY_INPUT_METHOD_MOUSE);
            overlay.set_overlay_flag(
                main_handle,
                VROVERLAY_FLAGS_SEND_VR_SMOOTH_SCROLL_EVENTS,
                true,
            );
            log::info!("SteamVR overlay: overlay configured");
        }
    }
    log::info!("SteamVR overlay: step 8 done");

    // ---- Step 9: Generate and set initial texture ----
    log::info!("SteamVR overlay: step 9 - setting texture...");
    let initial_lang = language
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| "en".to_string());
    let initial_tex = texture::generate(false, &initial_lang, false);
    if std::fs::write(&tex_path, &initial_tex).is_ok() {
        let tex_cstr = std::ffi::CString::new(tex_path.to_string_lossy().as_bytes()).unwrap();
        let tex_ptr = tex_cstr.as_ptr() as *const u8;
        unsafe {
            if thumb_handle != 0 {
                log::info!("SteamVR overlay: step 9a - SetOverlayFromFile on thumb...");
                overlay.set_overlay_from_file(thumb_handle, tex_ptr);
                log::info!("SteamVR overlay: thumbnail texture set");
            }
            if main_handle != 0 {
                log::info!("SteamVR overlay: step 9b - SetOverlayFromFile on main...");
                overlay.set_overlay_from_file(main_handle, tex_ptr);
                log::info!("SteamVR overlay: main texture set");
            }
        }
    } else {
        log::error!("SteamVR overlay: failed to write texture file");
    }
    log::info!("SteamVR overlay: step 9 done");

    // ---- Step 10: Main loop ----
    log::info!("SteamVR overlay: entering main loop");
    let active_set = VRActiveActionSet {
        ul_action_set: action_set_handle,
        ul_restricted_to_device: 0,
        ul_secondary_action_set: 0,
        un_padding: 0,
        n_priority: 0,
    };
    let mut was_recording = is_recording.load(Ordering::Relaxed);
    let mut was_lang = language
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| "en".to_string());
    // Track whether a binding exists (for overlay texture hint).
    let mut has_binding = false;
    let mut was_has_binding = false;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(11));
        if !enabled.load(Ordering::Relaxed) {
            log::info!("SteamVR overlay disabled — shutting down");
            unsafe {
                (dll.vr_shutdown)();
            }
            return;
        }

        // Poll overlay events
        if main_handle != 0 {
            let mut event: VREvent = unsafe { std::mem::zeroed() };
            unsafe {
                while overlay.poll_next_overlay_event(
                    main_handle,
                    &mut event,
                    std::mem::size_of::<VREvent>() as u32,
                ) {
                    match event.event_type {
                        VREVENT_QUIT => {
                            log::info!("SteamVR overlay: VREvent_Quit — shutting down");
                            unsafe {
                                (dll.vr_shutdown)();
                            }
                            // Follow SteamVR lifecycle: exit app when SteamVR quits
                            if auto_launch.load(Ordering::Relaxed) {
                                log::info!("SteamVR overlay: auto_launch enabled, exiting app");
                                std::process::exit(0);
                            }
                            return;
                        }
                        VREVENT_DASHBOARD_ACTIVATED => {
                            log::info!("SteamVR overlay: dashboard activated");
                        }
                        _ => {}
                    }
                }
            }
        }

        // Poll toggle_record action
        if toggle_action != 0 {
            unsafe {
                input.update_action_state(
                    &active_set,
                    std::mem::size_of::<VRActiveActionSet>() as u32,
                    1,
                );
            }

            let mut data: InputDigitalActionData = unsafe { std::mem::zeroed() };
            unsafe {
                input.get_digital_action_data(
                    toggle_action,
                    &mut data,
                    std::mem::size_of::<InputDigitalActionData>() as u32,
                    0,
                );
            }

            if data.b_state && data.b_changed {
                log::info!("SteamVR overlay: toggle_record pressed via controller");
                let currently = is_recording.load(Ordering::Relaxed);
                let cmd = if currently {
                    RenderCommand::StopRecording
                } else {
                    RenderCommand::StartRecording
                };
                if let Err(e) = cmd_tx.send(cmd) {
                    log::error!("SteamVR overlay: failed to send command: {e}");
                    break;
                }
            }

            // Track binding status for overlay hint
            has_binding = data.b_active;
        }

        // Process overlay commands from frontend
        while let Ok(cmd) = overlay_cmd_rx.try_recv() {
            match cmd {
                OverlayCommand::OpenBindingUI => {
                    log::info!("SteamVR overlay: opening binding UI (user requested)");
                    unsafe {
                        let err = input.open_binding_ui(
                            std::ptr::null(),       // null = current app
                            action_set_handle,      // jump to our action set
                            0,                      // no specific device
                            true,                   // show on desktop
                        );
                        if err != 0 {
                            log::warn!("SteamVR overlay: OpenBindingUI returned error={err}");
                        }
                    }
                }
            }
        }

        // Update texture when recording state, language, or binding status changes
        let now_recording = is_recording.load(Ordering::Relaxed);
        let now_lang = language
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| "en".to_string());
        let has_binding_changed = has_binding != was_has_binding;
        if now_recording != was_recording || now_lang != was_lang || has_binding_changed {
            was_recording = now_recording;
            was_lang = now_lang.clone();
            was_has_binding = has_binding;
            let tex = texture::generate(now_recording, &now_lang, has_binding);
            if std::fs::write(&tex_path, &tex).is_ok() {
                let cstr = std::ffi::CString::new(tex_path.to_string_lossy().as_bytes()).unwrap();
                let ptr = cstr.as_ptr() as *const u8;
                unsafe {
                    if main_handle != 0 {
                        overlay.set_overlay_from_file(main_handle, ptr);
                    }
                    if thumb_handle != 0 {
                        overlay.set_overlay_from_file(thumb_handle, ptr);
                    }
                }
                log::info!(
                    "SteamVR overlay: texture updated — {}",
                    if now_recording { "RECORDING" } else { "READY" }
                );
            }
        }
    }

    log::info!("SteamVR overlay: main loop exited");
    unsafe {
        (dll.vr_shutdown)();
    }
}

fn write_manifest(
    manifest_path: &std::path::Path,
    action_manifest_path: &std::path::Path,
) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let working_dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
    let json = format!(
        r#"{{
    "source": "builtin",
    "applications": [
        {{
            "app_key": "{APP_KEY}",
            "launch_type": "binary",
            "binary_path_windows": "{}",
            "working_directory": "{}",
            "action_manifest_path": "{}",
            "is_dashboard_overlay": true,
            "strings": {{
                "en_us": {{
                    "name": "VRCRecorder",
                    "description": "Record Spout video streams in VR"
                }}
            }}
        }}
    ]
}}
"#,
        json_path(&exe),
        json_path(working_dir),
        json_path(action_manifest_path),
    );
    std::fs::write(manifest_path, json)
}

fn json_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

/// Register the SteamVR application manifest through IVRApplications.
fn register_application_manifest(
    applications: &ffi::VRApplications,
    manifest_path: &std::path::Path,
) {
    let _app_key = std::ffi::CString::new(APP_KEY).unwrap();
    let manifest = std::ffi::CString::new(manifest_path.to_string_lossy().as_bytes()).unwrap();

    unsafe {
        // Always (re-)register — OpenVR deduplicates by app_key internally.
        // This ensures the manifest path is up-to-date after upgrades or path changes.
        let err = applications.add_application_manifest(manifest.as_ptr() as *const u8, false);
        if err == 0 {
            log::info!("SteamVR overlay: application manifest registered");
        } else {
            log::warn!("SteamVR overlay: AddApplicationManifest returned error={err}");
        }
    }
}

/// Fallback: manually register the manifest in SteamVR's appconfig.json.
fn register_in_appconfig(manifest_path: &std::path::Path) {
    // Find Steam config dir
    let steam_config = match steam_install_path() {
        Some(p) => p.join("config").join("appconfig.json"),
        None => {
            // Try common paths
            let candidates = [
                r"C:\Program Files (x86)\Steam\config\appconfig.json",
                r"D:\Steam\config\appconfig.json",
            ];
            let mut found = None;
            for c in &candidates {
                let p = std::path::PathBuf::from(c);
                if p.exists() {
                    found = Some(p);
                    break;
                }
            }
            match found {
                Some(p) => p,
                None => {
                    log::warn!("SteamVR overlay: could not find appconfig.json");
                    return;
                }
            }
        }
    };

    let manifest_str = manifest_path.to_string_lossy().to_string();
    // Normalize to forward slashes for JSON
    let manifest_str = manifest_str.replace('\\', "/");

    match std::fs::read_to_string(&steam_config) {
        Ok(content) => {
            // Check if our manifest is already registered
            if content.contains(&manifest_str) {
                log::info!("SteamVR overlay: manifest already in appconfig.json");
                return;
            }

            // Simple insertion: append to manifest_paths array
            // Look for the closing ] of manifest_paths
            if let Some(paths_end) = content.rfind(']') {
                let mut new_content = content[..paths_end].to_string();
                if !new_content.trim_end().ends_with('[') && !new_content.trim_end().ends_with(',')
                {
                    new_content.push(',');
                }
                new_content.push_str(&format!("\n        \"{}\"", manifest_str));
                new_content.push_str(&content[paths_end..]);

                if let Err(e) = std::fs::write(&steam_config, &new_content) {
                    log::warn!("SteamVR overlay: failed to write appconfig.json: {e}");
                } else {
                    log::info!(
                        "SteamVR overlay: manifest added to appconfig.json. Restart SteamVR to apply."
                    );
                }
            } else {
                log::warn!("SteamVR overlay: could not parse appconfig.json structure");
            }
        }
        Err(e) => {
            log::warn!("SteamVR overlay: cannot read appconfig.json: {e}");
        }
    }
}

/// Find Steam install path from registry.
fn steam_install_path() -> Option<std::path::PathBuf> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKEY_CURRENT_USER\Software\Valve\Steam",
            "/v",
            "SteamPath",
        ])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(idx) = line.find("REG_SZ") {
            let path = line[idx + 6..].trim();
            return Some(std::path::PathBuf::from(path));
        }
    }
    None
}
