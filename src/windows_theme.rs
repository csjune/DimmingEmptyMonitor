use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::addr_of_mut;
use std::sync::OnceLock;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HMODULE, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, RRF_RT_REG_DWORD,
    RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};
use windows::core::{PCSTR, PCWSTR, w};

const PERSONALIZE_KEY: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
const SYSTEM_THEME_VALUE: PCWSTR = w!("SystemUsesLightTheme");
const APPS_THEME_VALUE: PCWSTR = w!("AppsUseLightTheme");

static UXTHEME_MODULE: OnceLock<isize> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowsTheme {
    Light,
    Dark,
}

impl WindowsTheme {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub(crate) fn toggle_label(self) -> &'static str {
        match self {
            Self::Light => "Windows Dark Mode",
            Self::Dark => "Windows Light Mode",
        }
    }

    pub(crate) fn read() -> Result<Self, String> {
        let mut value = 1u32;
        let mut value_size = size_of::<u32>() as u32;
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PERSONALIZE_KEY,
                SYSTEM_THEME_VALUE,
                RRF_RT_REG_DWORD,
                None,
                Some(addr_of_mut!(value).cast()),
                Some(&mut value_size),
            )
        };

        if result == ERROR_SUCCESS {
            Ok(if value == 0 { Self::Dark } else { Self::Light })
        } else if result == ERROR_FILE_NOT_FOUND {
            Ok(Self::Light)
        } else {
            Err(format!(
                "Failed to read Windows theme setting (error {})",
                result.0
            ))
        }
    }

    pub(crate) fn apply(self) -> Result<(), String> {
        let mut key = HKEY::default();
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PERSONALIZE_KEY,
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        if result != ERROR_SUCCESS {
            return Err(format!(
                "Failed to open Windows theme settings (error {})",
                result.0
            ));
        }

        let value = u32::from(self == Self::Light).to_ne_bytes();
        let system_result =
            unsafe { RegSetValueExW(key, SYSTEM_THEME_VALUE, None, REG_DWORD, Some(&value)) };
        let apps_result =
            unsafe { RegSetValueExW(key, APPS_THEME_VALUE, None, REG_DWORD, Some(&value)) };
        unsafe {
            let _ = RegCloseKey(key);
        }

        if system_result != ERROR_SUCCESS {
            return Err(format!(
                "Failed to change Windows theme setting (error {})",
                system_result.0
            ));
        }
        if apps_result != ERROR_SUCCESS {
            return Err(format!(
                "Failed to change Windows app theme setting (error {})",
                apps_result.0
            ));
        }

        broadcast_theme_change();
        Ok(())
    }
}

pub(crate) fn enable_native_dark_menus() {
    type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;

    unsafe {
        let Some(library) = uxtheme_module() else {
            return;
        };

        if let Some(set_preferred_app_mode) = GetProcAddress(library, PCSTR(135usize as *const u8))
        {
            let set_preferred_app_mode: SetPreferredAppMode =
                std::mem::transmute(set_preferred_app_mode);
            const PREFERRED_APP_MODE_ALLOW_DARK: i32 = 1;
            let _ = set_preferred_app_mode(PREFERRED_APP_MODE_ALLOW_DARK);
        }

        flush_native_menu_themes(library);
    }
}

pub(crate) fn refresh_native_menu_theme() {
    unsafe {
        let Some(library) = uxtheme_module() else {
            return;
        };

        flush_native_menu_themes(library);
    }
}

fn broadcast_theme_change() {
    let setting_name = wide_null("ImmersiveColorSet");
    let mut broadcast_result = 0usize;
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(setting_name.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            250,
            Some(&mut broadcast_result),
        );
    }
}

unsafe fn flush_native_menu_themes(library: HMODULE) {
    type FlushMenuThemes = unsafe extern "system" fn();

    if let Some(flush_menu_themes) = GetProcAddress(library, PCSTR(136usize as *const u8)) {
        let flush_menu_themes: FlushMenuThemes = std::mem::transmute(flush_menu_themes);
        flush_menu_themes();
    }
}

fn uxtheme_module() -> Option<HMODULE> {
    if let Some(handle) = UXTHEME_MODULE.get() {
        return Some(HMODULE(*handle as *mut c_void));
    }

    let library = unsafe { LoadLibraryW(w!("uxtheme.dll")) }.ok()?;
    let _ = UXTHEME_MODULE.set(library.0 as isize);
    Some(library)
}

fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_label_names_the_target_mode() {
        assert_eq!(WindowsTheme::Light.toggle_label(), "Windows Dark Mode");
        assert_eq!(WindowsTheme::Dark.toggle_label(), "Windows Light Mode");
    }
}
