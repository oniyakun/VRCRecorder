//! Raw FFI bindings for OpenVR C API (openvr_capi.h).
//! Stores the fn table base as usize, reads 8-byte function pointers at known indices.

use std::path::PathBuf;

pub const FNTABLE_PREFIX: &str = "FnTable:";

// ===== Primitive types =====

pub type VROverlayHandle = u64;
pub type VRActionHandle = u64;
pub type VRInputValueHandle = u64;

// ===== Enum constants =====

pub const VR_APPLICATION_OVERLAY: i32 = 2;
pub const VR_APPLICATION_BACKGROUND: i32 = 1;

// ===== VREvent_t =====

pub const VREVENT_SIZE: usize = 512;

#[repr(C, align(8))]
pub struct VREvent {
    pub event_type: u32,
    _pad: [u8; VREVENT_SIZE - 4],
}

pub const VREVENT_QUIT: u32 = 201;
pub const VREVENT_DASHBOARD_ACTIVATED: u32 = 802;

// ===== InputDigitalActionData_t =====

#[repr(C)]
pub struct InputDigitalActionData {
    pub b_active: bool,
    pub active_origin: VRInputValueHandle,
    pub b_state: bool,
    pub b_changed: bool,
    pub f_update_time: f32,
}

// ===== VRActiveActionSet =====

#[repr(C)]
pub struct VRActiveActionSet {
    pub ul_action_set: VRActionHandle,
    pub ul_restricted_to_device: VRInputValueHandle,
    pub ul_secondary_action_set: VRActionHandle,
    pub un_padding: u32,
    pub n_priority: i32,
}

// ===== Fn table indices (0-based, verified against openvr_capi.h from OpenVR-AdvancedSettings SDK) =====

// IVROverlay
const OV_DESTROY_OVERLAY: isize = 2;
const OV_SET_OVERLAY_FLAG: isize = 10;
const OV_SET_OVERLAY_WIDTH: isize = 21;
const OV_SHOW_OVERLAY: isize = 41;
const OV_HIDE_OVERLAY: isize = 42;
const OV_POLL_NEXT_OVERLAY_EVENT: isize = 46;
const OV_SET_OVERLAY_INPUT_METHOD: isize = 48;
const OV_SET_OVERLAY_FROM_FILE: isize = 61;
const OV_CREATE_DASHBOARD_OVERLAY: isize = 65;

// IVRInput
const IN_SET_ACTION_MANIFEST_PATH: isize = 0;
const IN_GET_ACTION_SET_HANDLE: isize = 1;
const IN_GET_ACTION_HANDLE: isize = 2;
const IN_UPDATE_ACTION_STATE: isize = 4;
const IN_GET_DIGITAL_ACTION_DATA: isize = 5;

// IVRApplications
const APP_ADD_APPLICATION_MANIFEST: isize = 0;
const APP_IS_APPLICATION_INSTALLED: isize = 2;

// Interface version strings
pub const IVROVERLAY_VERSION: &str = "IVROverlay_027";
pub const IVRINPUT_VERSION: &str = "IVRInput_010";
pub const IVRAPPLICATIONS_VERSION: &str = "IVRApplications_007";

// ===== Fn table access =====

/// Represents an OpenVR fn table. Stores the base address as usize.
/// On x64, each fn table element is an 8-byte function pointer.
pub struct FnTable {
    base: usize,
}

impl FnTable {
    pub unsafe fn new(ptr: *mut std::ffi::c_void) -> Self {
        Self { base: ptr as usize }
    }

    /// Read a function pointer at the given index.
    /// Returns the raw 8-byte value (the function address).
    pub unsafe fn read_fn(&self, index: isize) -> usize {
        unsafe {
            let addr = (self.base as *const usize).offset(index);
            *addr
        }
    }
}

// ===== IVROverlay wrapper =====

pub struct VROverlay {
    table: FnTable,
}

macro_rules! ov_call {
    ($self:expr, $index:expr, $ret:ty, ($($arg:ty),*), $($val:expr),*) => {{
        let raw = unsafe { $self.table.read_fn($index) };
        let f: unsafe extern "system" fn($($arg),*) -> $ret = unsafe { std::mem::transmute(raw) };
        unsafe { f($($val),*) }
    }};
}

impl VROverlay {
    pub unsafe fn new(ptr: *mut std::ffi::c_void) -> Self {
        Self {
            table: unsafe { FnTable::new(ptr) },
        }
    }

    pub unsafe fn destroy_overlay(&self, handle: VROverlayHandle) -> i32 {
        ov_call!(self, OV_DESTROY_OVERLAY, i32, (VROverlayHandle), handle)
    }

    pub unsafe fn set_overlay_flag(&self, handle: VROverlayHandle, flag: i32, enabled: bool) -> i32 {
        ov_call!(self, OV_SET_OVERLAY_FLAG, i32, (VROverlayHandle, i32, bool), handle, flag, enabled)
    }

    pub unsafe fn set_overlay_width_in_meters(&self, handle: VROverlayHandle, width: f32) -> i32 {
        ov_call!(self, OV_SET_OVERLAY_WIDTH, i32, (VROverlayHandle, f32), handle, width)
    }

    pub unsafe fn show_overlay(&self, handle: VROverlayHandle) -> i32 {
        ov_call!(self, OV_SHOW_OVERLAY, i32, (VROverlayHandle), handle)
    }

    pub unsafe fn hide_overlay(&self, handle: VROverlayHandle) -> i32 {
        ov_call!(self, OV_HIDE_OVERLAY, i32, (VROverlayHandle), handle)
    }

    pub unsafe fn poll_next_overlay_event(
        &self, handle: VROverlayHandle, event: *mut VREvent, size: u32,
    ) -> bool {
        ov_call!(self, OV_POLL_NEXT_OVERLAY_EVENT, bool, (VROverlayHandle, *mut VREvent, u32), handle, event, size)
    }

    pub unsafe fn set_overlay_input_method(&self, handle: VROverlayHandle, method: i32) -> i32 {
        ov_call!(self, OV_SET_OVERLAY_INPUT_METHOD, i32, (VROverlayHandle, i32), handle, method)
    }

    pub unsafe fn set_overlay_from_file(&self, handle: VROverlayHandle, path: *const u8) -> i32 {
        ov_call!(self, OV_SET_OVERLAY_FROM_FILE, i32, (VROverlayHandle, *const u8), handle, path)
    }

    pub unsafe fn create_dashboard_overlay(
        &self, key: *const u8, name: *const u8, main: *mut VROverlayHandle, thumb: *mut VROverlayHandle,
    ) -> i32 {
        ov_call!(self, OV_CREATE_DASHBOARD_OVERLAY, i32, (*const u8, *const u8, *mut VROverlayHandle, *mut VROverlayHandle), key, name, main, thumb)
    }
}

// ===== IVRInput wrapper =====

macro_rules! in_call {
    ($self:expr, $index:expr, $ret:ty, ($($arg:ty),*), $($val:expr),*) => {{
        let raw = unsafe { $self.table.read_fn($index) };
        let f: unsafe extern "system" fn($($arg),*) -> $ret = unsafe { std::mem::transmute(raw) };
        unsafe { f($($val),*) }
    }};
}

pub struct VRInput {
    table: FnTable,
}

impl VRInput {
    pub unsafe fn new(ptr: *mut std::ffi::c_void) -> Self {
        Self {
            table: unsafe { FnTable::new(ptr) },
        }
    }

    pub unsafe fn set_action_manifest_path(&self, path: *const u8) -> i32 {
        in_call!(self, IN_SET_ACTION_MANIFEST_PATH, i32, (*const u8), path)
    }

    pub unsafe fn get_action_set_handle(&self, name: *const u8, handle: *mut VRActionHandle) -> i32 {
        in_call!(self, IN_GET_ACTION_SET_HANDLE, i32, (*const u8, *mut VRActionHandle), name, handle)
    }

    pub unsafe fn get_action_handle(&self, name: *const u8, handle: *mut VRActionHandle) -> i32 {
        in_call!(self, IN_GET_ACTION_HANDLE, i32, (*const u8, *mut VRActionHandle), name, handle)
    }

    pub unsafe fn update_action_state(
        &self, sets: *const VRActiveActionSet, struct_size: u32, count: u32,
    ) -> i32 {
        in_call!(self, IN_UPDATE_ACTION_STATE, i32, (*const VRActiveActionSet, u32, u32), sets, struct_size, count)
    }

    pub unsafe fn get_digital_action_data(
        &self, action: VRActionHandle, data: *mut InputDigitalActionData, data_size: u32, restrict_to_device: VRInputValueHandle,
    ) -> i32 {
        in_call!(self, IN_GET_DIGITAL_ACTION_DATA, i32, (VRActionHandle, *mut InputDigitalActionData, u32, VRInputValueHandle), action, data, data_size, restrict_to_device)
    }
}

// ===== IVRApplications wrapper =====

macro_rules! app_call {
    ($self:expr, $index:expr, $ret:ty, ($($arg:ty),*), $($val:expr),*) => {{
        let raw = unsafe { $self.table.read_fn($index) };
        let f: unsafe extern "system" fn($($arg),*) -> $ret = unsafe { std::mem::transmute(raw) };
        unsafe { f($($val),*) }
    }};
}

pub struct VRApplications {
    table: FnTable,
}

impl VRApplications {
    pub unsafe fn new(ptr: *mut std::ffi::c_void) -> Self {
        Self {
            table: unsafe { FnTable::new(ptr) },
        }
    }

    pub unsafe fn add_application_manifest(&self, path: *const u8, temporary: bool) -> i32 {
        app_call!(self, APP_ADD_APPLICATION_MANIFEST, i32, (*const u8, bool), path, temporary)
    }

    pub unsafe fn is_application_installed(&self, app_key: *const u8) -> bool {
        app_call!(self, APP_IS_APPLICATION_INSTALLED, bool, (*const u8), app_key)
    }
}

// ===== DLL-level types =====

type VrInitFn = unsafe extern "system" fn(*mut i32, i32) -> *mut std::ffi::c_void;
type VrShutdownFn = unsafe extern "system" fn();
type VrGetGenericInterfaceFn = unsafe extern "system" fn(*const u8, *mut i32) -> *mut std::ffi::c_void;

pub struct OpenVRDll {
    _lib: *mut std::ffi::c_void,
    pub vr_init: VrInitFn,
    pub vr_shutdown: VrShutdownFn,
    pub vr_get_generic_interface: VrGetGenericInterfaceFn,
}

// ===== DLL loading =====

pub fn find_openvr_dll() -> Option<PathBuf> {
    let steam_path = steam_install_path()?;
    let steamvr_dll = steam_path
        .join("steamapps")
        .join("common")
        .join("SteamVR")
        .join("bin")
        .join("win64")
        .join("openvr_api.dll");
    if steamvr_dll.exists() {
        return Some(steamvr_dll);
    }
    for lib_dir in &[r"C:\Program Files (x86)\Steam", r"D:\Steam", r"E:\Steam"] {
        let dll = PathBuf::from(lib_dir)
            .join("steamapps/common/SteamVR/bin/win64/openvr_api.dll");
        if dll.exists() {
            return Some(dll);
        }
    }
    None
}

fn steam_install_path() -> Option<PathBuf> {
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
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// Launch SteamVR via its `vrstartup.exe` (located next to `openvr_api.dll`).
/// Returns Err if SteamVR can't be located or the process fails to spawn.
pub fn launch_steamvr() -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let dll_path = find_openvr_dll()
        .ok_or_else(|| "Cannot launch SteamVR: openvr_api.dll not found".to_string())?;
    // openvr_api.dll lives in <SteamVR>/bin/win64/, alongside vrstartup.exe.
    let vrstartup = dll_path
        .parent()
        .ok_or_else(|| "Cannot resolve SteamVR bin directory".to_string())?
        .join("vrstartup.exe");
    if !vrstartup.exists() {
        return Err(format!("vrstartup.exe not found at {}", vrstartup.display()));
    }

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    std::process::Command::new(&vrstartup)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to launch SteamVR: {e}"))?;
    Ok(())
}

pub fn load_openvr() -> Result<OpenVRDll, String> {
    use std::os::windows::ffi::OsStrExt;

    let dll_path = find_openvr_dll()
        .ok_or_else(|| "openvr_api.dll not found. Is SteamVR installed?".to_string())?;

    let wide: Vec<u16> = dll_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let lib = unsafe { LoadLibraryW(wide.as_ptr()) };
    if lib.is_null() {
        return Err(format!("Failed to load openvr_api.dll from {}", dll_path.display()));
    }

    macro_rules! resolve {
        ($lib:expr, $name:expr) => {
            unsafe {
                let name_bytes = std::ffi::CString::new($name).unwrap();
                let ptr = GetProcAddress($lib, name_bytes.as_ptr());
                if ptr.is_none() {
                    return Err(format!("Failed to resolve {}", $name));
                }
                std::mem::transmute(ptr)
            }
        };
    }

    Ok(OpenVRDll {
        _lib: lib,
        vr_init: resolve!(lib, "VR_InitInternal"),
        vr_shutdown: resolve!(lib, "VR_ShutdownInternal"),
        vr_get_generic_interface: resolve!(lib, "VR_GetGenericInterface"),
    })
}

unsafe extern "system" {
    fn LoadLibraryW(lpLibFileName: *const u16) -> *mut std::ffi::c_void;
    fn GetProcAddress(hModule: *mut std::ffi::c_void, lpProcName: *const i8) -> Option<unsafe extern "C" fn()>;
}

pub fn get_interface(dll: &OpenVRDll, name: &str) -> Result<*mut std::ffi::c_void, String> {
    let c_name = std::ffi::CString::new(format!("{FNTABLE_PREFIX}{name}"))
        .map_err(|_| "Invalid interface name".to_string())?;
    let mut error: i32 = 0;
    let ptr = unsafe { (dll.vr_get_generic_interface)(c_name.as_ptr() as *const u8, &mut error) };
    if ptr.is_null() {
        Err(format!("VR_GetGenericInterface({FNTABLE_PREFIX}{name}) failed, error={error}"))
    } else {
        Ok(ptr)
    }
}
