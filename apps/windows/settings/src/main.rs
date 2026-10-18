//! LucidSettings.exe：配置 AI 服务，并完成当前用户 TSF 注册。

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod identity;
#[cfg(windows)]
mod registration;
#[cfg(windows)]
mod settings;
#[cfg(windows)]
mod settings_window;
#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--register") => {
            let Some(path) = args.next() else {
                eprintln!("用法：LucidSettings.exe --register <LucidTextService.dll>");
                std::process::exit(2);
            };
            finish(registration::register_text_service(std::path::Path::new(
                &path,
            )));
        }
        Some("--unregister") => finish(registration::unregister_text_service()),
        Some("--help") | Some("-h") => println!(
            "Lucid Windows\n\n  LucidSettings.exe\n      打开设置\n  LucidSettings.exe --register <LucidTextService.dll>\n      注册英语文本服务\n  LucidSettings.exe --unregister\n      移除文本服务"
        ),
        Some(other) => {
            eprintln!("无法识别的参数：{other}");
            std::process::exit(2);
        }
        None => finish(settings_window::run()),
    }
}

#[cfg(windows)]
fn finish(result: Result<(), String>) {
    if let Err(error) = result {
        win32::show_error(&error);
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LucidSettings 只能在 Windows 上运行。");
}
