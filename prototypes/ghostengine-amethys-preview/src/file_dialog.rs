//! Native image-file picker used for user-owned game artwork.
use std::path::PathBuf;

/// Opens a native image picker. Cancellation returns Ok(None).
#[cfg(windows)]
pub fn pick_image() -> Result<Option<PathBuf>, String> {
    use std::{ffi::c_void, mem::size_of};
    #[repr(C)]
    struct OpenFileNameW {
        struct_size: u32, owner: *mut c_void, instance: *mut c_void,
        filter: *const u16, custom_filter: *mut u16, max_custom_filter: u32,
        filter_index: u32, file: *mut u16, max_file: u32,
        file_title: *mut u16, max_file_title: u32, initial_dir: *const u16,
        title: *const u16, flags: u32, file_offset: u16, file_extension: u16,
        default_extension: *const u16, custom_data: isize, hook: *mut c_void,
        template_name: *const u16, reserved: *mut c_void, reserved_word: u32, flags_ex: u32,
    }
    #[link(name = "comdlg32")]
    extern "system" {
        fn GetOpenFileNameW(file_name: *mut OpenFileNameW) -> i32;
        fn CommDlgExtendedError() -> u32;
    }
    let filter: Vec<u16> = "Images\0*.png;*.jpg;*.jpeg;*.bmp\0Tous les fichiers\0*.*\0\0".encode_utf16().collect();
    let title: Vec<u16> = "Choisir une jaquette pour Amethys\0".encode_utf16().collect();
    let mut file = vec![0u16; 32_768];
    let mut dialog = OpenFileNameW {
        struct_size: size_of::<OpenFileNameW>() as u32,
        owner: std::ptr::null_mut(), instance: std::ptr::null_mut(),
        filter: filter.as_ptr(), custom_filter: std::ptr::null_mut(), max_custom_filter: 0,
        filter_index: 1, file: file.as_mut_ptr(), max_file: file.len() as u32,
        file_title: std::ptr::null_mut(), max_file_title: 0,
        initial_dir: std::ptr::null(), title: title.as_ptr(),
        flags: 0x0000_1000 | 0x0000_0800 | 0x0000_0008,
        file_offset: 0, file_extension: 0, default_extension: std::ptr::null(),
        custom_data: 0, hook: std::ptr::null_mut(), template_name: std::ptr::null(),
        reserved: std::ptr::null_mut(), reserved_word: 0, flags_ex: 0,
    };
    let selected = unsafe { GetOpenFileNameW(&mut dialog) };
    if selected == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 { Ok(None) } else { Err(format!("erreur Windows du sélecteur (0x{error:08X})")) };
    }
    let length = file.iter().position(|unit| *unit == 0).unwrap_or(file.len());
    Ok(Some(PathBuf::from(String::from_utf16(&file[..length]).map_err(|error| error.to_string())?)))
}

#[cfg(target_os = "linux")]
pub fn pick_image() -> Result<Option<PathBuf>, String> {
    use std::process::Command;
    let zenity = Command::new("zenity")
        .args(["--file-selection", "--title=Choisir une jaquette", "--file-filter=Images | *.png *.jpg *.jpeg *.bmp"])
        .output();
    if let Ok(output) = zenity {
        if output.status.success() {
            return Ok(Some(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())));
        }
        return Ok(None);
    }
    let output = Command::new("kdialog")
        .args(["--getopenfilename", ".", "*.png *.jpg *.jpeg *.bmp|Images"])
        .output()
        .map_err(|error| format!("ni zenity ni kdialog ne sont disponibles : {error}"))?;
    if output.status.success() {
        Ok(Some(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())))
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
pub fn pick_image() -> Result<Option<PathBuf>, String> {
    use std::process::Command;
    let output = Command::new("osascript")
        .args(["-e", "POSIX path of (choose file with prompt \"Choisir une jaquette\")"])
        .output()
        .map_err(|error| format!("impossible d'ouvrir le sélecteur macOS : {error}"))?;
    if output.status.success() {
        Ok(Some(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())))
    } else {
        Ok(None)
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn pick_image() -> Result<Option<PathBuf>, String> {
    Err("sélecteur de fichiers non pris en charge sur cette plateforme".to_owned())
}
