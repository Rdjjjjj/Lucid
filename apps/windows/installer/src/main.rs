//! Lucid Windows 单文件安装程序。
//!
//! 组件先由 workspace 构建，再通过 include_bytes! 嵌入本程序，用户只需要双击
//! LucidSetup.exe；安装过程不依赖 PowerShell、脚本或额外运行时。

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(windows)]
use windows::{
    Win32::{
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegSetValueExW,
        },
        UI::WindowsAndMessaging::{
            MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MESSAGEBOX_STYLE, MessageBoxW,
        },
    },
    core::PCWSTR,
};

#[cfg(windows)]
const SERVER_EXE: &[u8] =
    include_bytes!("../../../../target/x86_64-pc-windows-gnu/release/LucidServer.exe");
#[cfg(windows)]
const SETTINGS_EXE: &[u8] =
    include_bytes!("../../../../target/x86_64-pc-windows-gnu/release/LucidSettings.exe");
#[cfg(windows)]
const TEXT_SERVICE_DLL: &[u8] =
    include_bytes!("../../../../target/x86_64-pc-windows-gnu/release/LucidTextService.dll");
#[cfg(windows)]
const SETTINGS_MANIFEST: &[u8] = include_bytes!("../LucidSettings.exe.manifest");

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_VALUE: &str = "LucidServer";

#[cfg(windows)]
fn main() {
    match install() {
        Ok(destination) => {
            let settings = destination.join("LucidSettings.exe");
            let _ = launch_user_process(&settings);
            message(
                "Lucid 安装完成。\n\n设置窗口已经打开，请配置 AI 服务和模型。\n配置完成后，在 Windows 输入法设置中添加 Lucid。",
                "Lucid",
                MB_OK | MB_ICONINFORMATION,
            );
        }
        Err(error) => message(
            &format!("Lucid 安装失败：\n\n{error}"),
            "Lucid 安装程序",
            MB_OK | MB_ICONERROR,
        ),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LucidSetup 只能在 Windows 上运行。");
}

#[cfg(windows)]
fn install() -> Result<PathBuf, String> {
    let local_app_data = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "无法找到当前用户的 LOCALAPPDATA 目录。".to_owned())?;
    let destination = local_app_data.join("Lucid");

    stop_existing_server();
    fs::create_dir_all(&destination).map_err(|error| format!("无法创建安装目录：{error}"))?;
    write_payload(&destination.join("LucidServer.exe"), SERVER_EXE)?;
    write_payload(&destination.join("LucidSettings.exe"), SETTINGS_EXE)?;
    write_payload(&destination.join("LucidTextService.dll"), TEXT_SERVICE_DLL)?;
    write_payload(
        &destination.join("LucidSettings.exe.manifest"),
        SETTINGS_MANIFEST,
    )?;

    let settings = destination.join("LucidSettings.exe");
    let service = destination.join("LucidTextService.dll");
    let status = Command::new(&settings)
        .arg("--register")
        .arg(&service)
        .status()
        .map_err(|error| format!("无法启动 TSF 注册程序：{error}"))?;
    if !status.success() {
        return Err(format!(
            "TSF 注册失败，退出码：{}。",
            status.code().unwrap_or(-1)
        ));
    }

    let server = destination.join("LucidServer.exe");
    set_startup_command(&server)?;
    launch_user_process(&server).map_err(|error| format!("无法启动 LucidServer：{error}"))?;

    Ok(destination)
}

#[cfg(windows)]
fn launch_user_process(path: &Path) -> std::io::Result<()> {
    // The installer is elevated for TSF registration. Asking the existing
    // Explorer shell to open the executable starts it back at normal user
    // integrity, so the Server can talk to ordinary desktop applications.
    Command::new("explorer.exe").arg(path).spawn().map(|_| ())
}

#[cfg(windows)]
fn write_payload(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|error| format!("无法写入 {}：{error}", path.display()))
}

#[cfg(windows)]
fn stop_existing_server() {
    let _ = Command::new("taskkill")
        .args(["/IM", "LucidServer.exe", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
fn set_startup_command(server: &Path) -> Result<(), String> {
    let key_path = wide(RUN_KEY);
    let value_name = wide(RUN_VALUE);
    let value = wide(&format!(r#""{}""#, server.display()));
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
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
        return Err("无法写入 Lucid 开机启动项。".to_owned());
    }

    let bytes = unsafe { std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), value.len() * 2) };
    let write_status =
        unsafe { RegSetValueExW(key, PCWSTR(value_name.as_ptr()), None, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if write_status.0 != 0 {
        return Err("无法保存 Lucid 开机启动项。".to_owned());
    }
    Ok(())
}

#[cfg(windows)]
fn message(text: &str, title: &str, flags: MESSAGEBOX_STYLE) {
    let text = wide(text);
    let title = wide(title);
    unsafe {
        let _ = MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), flags);
    }
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
