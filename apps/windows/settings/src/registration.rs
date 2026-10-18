//! 注册一个英语 TSF 文本服务。只写 Lucid 自己的 CLSID 和 Profile。

use std::path::Path;

use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
                RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
            },
        },
        UI::TextServices::{
            CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD,
            ITfCategoryMgr, ITfInputProcessorProfiles, TF_PROFILETYPE_INPUTPROCESSOR,
        },
    },
    core::{GUID, Interface, PCWSTR},
};

use crate::identity::{
    CLSID_TEXT_SERVICE, LANG_EN_US, PROFILE_DESCRIPTION, PROFILE_GUID, clsid_brace,
};
use crate::win32;

pub fn register_text_service(dll_path: &Path) -> Result<(), String> {
    // TSF registration is COM based.  The installer runs in a fresh process,
    // so there is no host application that has initialized COM for us.
    win32::initialize_com().map_err(|error| format!("无法初始化 COM：{error}"))?;
    let dll = dll_path
        .canonicalize()
        .unwrap_or_else(|_| dll_path.to_path_buf());
    if !dll.is_file() {
        return Err("找不到 LucidTextService.dll。".to_owned());
    }
    // Make installation repeatable after an interrupted or older registration.
    let _ = unregister_text_service();
    write_com_server(&dll)?;
    register_profile(&dll)?;
    if let Err(error) = register_keyboard_category() {
        let _ = unregister_text_service();
        return Err(error);
    }
    Ok(())
}

pub fn unregister_text_service() -> Result<(), String> {
    win32::initialize_com().map_err(|error| format!("无法初始化 COM：{error}"))?;
    let _ = remove_profile();
    let _ = remove_keyboard_category();
    delete_tree(
        HKEY_CURRENT_USER,
        &format!(r"Software\Classes\CLSID\{}", clsid_brace()),
    )?;
    Ok(())
}

fn write_com_server(dll: &Path) -> Result<(), String> {
    let clsid = clsid_brace();
    let classes = format!(r"Software\Classes\CLSID\{clsid}");
    let inproc = format!(r"{classes}\InprocServer32");
    set_string(HKEY_CURRENT_USER, &classes, "", "Lucid Text Service")?;
    set_string(HKEY_CURRENT_USER, &inproc, "", &dll.display().to_string())?;
    set_string(HKEY_CURRENT_USER, &inproc, "ThreadingModel", "Apartment")?;
    Ok(())
}

fn register_profile(dll: &Path) -> Result<(), String> {
    let profiles = profiles()?;
    let clsid = service_guid();
    let profile = profile_guid();
    let description = wide(PROFILE_DESCRIPTION);
    let icon = wide(&dll.display().to_string());
    unsafe {
        profiles
            .Register(&clsid)
            .map_err(|error| format!("无法注册文本服务：{error}"))?;
        profiles
            .AddLanguageProfile(&clsid, LANG_EN_US, &profile, &description, &icon, 0)
            .map_err(|error| format!("无法添加英语配置：{error}"))?;
        profiles
            .EnableLanguageProfile(&clsid, LANG_EN_US, &profile, true)
            .map_err(|error| format!("无法启用英语配置：{error}"))?;
    }
    let _ = TF_PROFILETYPE_INPUTPROCESSOR;
    Ok(())
}

fn remove_profile() -> Result<(), String> {
    let profiles = profiles()?;
    let clsid = service_guid();
    let profile = profile_guid();
    unsafe {
        let _ = profiles.RemoveLanguageProfile(&clsid, LANG_EN_US, &profile);
        profiles
            .Unregister(&clsid)
            .map_err(|error| format!("无法注销文本服务：{error}"))?;
    }
    Ok(())
}

fn register_keyboard_category() -> Result<(), String> {
    let categories = categories()?;
    let clsid = service_guid();
    let profile = profile_guid();
    unsafe {
        categories
            .RegisterCategory(&clsid, &GUID_TFCAT_TIP_KEYBOARD, &profile)
            .map_err(|error| format!("无法登记键盘文本服务类别：{error}"))
    }
}

fn remove_keyboard_category() -> Result<(), String> {
    let categories = categories()?;
    let clsid = service_guid();
    let profile = profile_guid();
    unsafe {
        categories
            .UnregisterCategory(&clsid, &GUID_TFCAT_TIP_KEYBOARD, &profile)
            .map_err(|error| error.to_string())
    }
}

fn profiles() -> Result<ITfInputProcessorProfiles, String> {
    com_object(&CLSID_TF_InputProcessorProfiles)
}

fn categories() -> Result<ITfCategoryMgr, String> {
    com_object(&CLSID_TF_CategoryMgr)
}

fn com_object<T: Interface>(clsid: &GUID) -> Result<T, String> {
    unsafe {
        CoCreateInstance(clsid, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("无法创建 TSF 注册对象：{error}"))
    }
}

pub fn service_guid() -> GUID {
    guid_from_text("{7C4E2A11-6B0D-4E58-9A31-1D5C8B0E4F27}")
}

pub fn profile_guid() -> GUID {
    guid_from_text("{A91F6C42-3D18-4B77-8E05-6C2A9D14B8E3}")
}

fn guid_from_text(text: &str) -> GUID {
    let bare = text.trim_matches(['{', '}']);
    bare.try_into().unwrap_or_else(|_| GUID::zeroed())
}

fn set_string(root: HKEY, path: &str, name: &str, value: &str) -> Result<(), String> {
    let subkey = wide(path);
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR(subkey.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if status.0 != 0 {
        return Err(format!("无法写入注册表 {path}。"));
    }
    let data = wide(value);
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2) };
    let value_name = wide(name);
    let written =
        unsafe { RegSetValueExW(key, PCWSTR(value_name.as_ptr()), None, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if written.0 != 0 {
        return Err(format!("无法写入注册表值 {path}\\{name}。"));
    }
    let _ = (CLSID_TEXT_SERVICE, PROFILE_GUID);
    Ok(())
}

fn delete_tree(root: HKEY, path: &str) -> Result<(), String> {
    let subkey = wide(path);
    let status = unsafe { RegDeleteTreeW(root, PCWSTR(subkey.as_ptr())) };
    if status.0 == 0 || status.0 == 2 {
        Ok(())
    } else {
        Err(format!("无法删除注册表项 {path}。"))
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
