//! Lucid macOS 输入法壳。
//!
//! 按键立刻交给当前应用。句子结束后，Core 判断是否请求 AI；建议只在用户确认后写回。
//! 这个壳不决定拼音怎么替换，也不解析模型响应。

mod app;
mod host;
mod imk;
mod preferences;
mod suggestion;

use objc2::{AnyThread, ClassType, MainThreadMarker};
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;
use objc2_input_method_kit::IMKServer;

fn main() {
    // The settings app is bundled with the same binary as the input method so
    // both pieces always ship together.  LaunchServices starts the settings
    // bundle by its executable name (`Lucid`), not by passing our private
    // `--settings` flag; recognize that name here so the bundle contains a
    // real Mach-O executable instead of a shell-script wrapper.
    let executable_name = std::env::args()
        .next()
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_stem()
                .map(|name| name.to_owned())
        })
        .and_then(|name| name.to_str().map(str::to_owned))
        .unwrap_or_default();
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.iter().any(|argument| argument == "--deactivate") {
        app::input_source::disable();
        return;
    }
    if arguments
        .iter()
        .any(|argument| argument == "--install" || argument == "--enable")
    {
        if let Err(error) = app::input_source::enable_and_select() {
            eprintln!("Lucid 输入源启用失败：{error}");
            std::process::exit(1);
        }
        return;
    }
    if arguments.iter().any(|argument| argument == "--select") {
        if let Err(error) = app::input_source::select() {
            eprintln!("Lucid 输入源选择失败：{error}");
            std::process::exit(1);
        }
        return;
    }
    // The settings bundle intentionally reuses the same Rust binary.  Do not
    // rely on a shell wrapper or on the executable name: LaunchServices may
    // launch the binary as `LucidInputMethod`, while the settings bundle is
    // identified by its own bundle id.  The old name-only check made the
    // installed app start as a headless input method and show no window.
    let bundle_info = app::bundle::BundleInfo::from_main_bundle();
    if executable_name == "Lucid"
        || bundle_info.identifier == "io.github.rdj.lucid"
        || arguments.iter().any(|argument| argument == "--settings")
    {
        preferences::run();
        return;
    }
    if arguments.iter().any(|argument| argument == "--self-check") {
        let _class = imk::LucidInputController::class();
        if let Err(error) = imk::verify_event_entries() {
            eprintln!("self-check: {error}");
            std::process::exit(1);
        }
        println!(
            "self-check: {} controller and all input entries verified",
            env!("CARGO_PKG_VERSION")
        );
        if !host::self_check_replacement() {
            eprintln!("self-check: replacement failed");
            std::process::exit(1);
        }
        println!("self-check: replacement verified");
        return;
    }

    let _log_guard = app::logging::init();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        build = "wechat-readback-3",
        "Lucid 输入法启动"
    );
    let _ = std::hint::black_box("lucid-build-marker:wechat-readback-3");
    std::panic::set_hook(Box::new(|info| {
        tracing::error!(%info, "panic");
    }));

    // define_class! 的类要先注册。IMKServer 按 Info.plist 找类，找不到会静默退回基类，按键全部透传。
    let controller_class = imk::LucidInputController::class();
    if let Err(error) = imk::verify_event_entries() {
        tracing::error!(%error, "输入法控制器验证失败");
        std::process::exit(1);
    }
    tracing::info!(class = ?controller_class.name(), "控制器和输入入口已注册");

    let info = bundle_info;
    let server = unsafe {
        IMKServer::initWithName_bundleIdentifier(
            IMKServer::alloc(),
            Some(&NSString::from_str(&info.connection_name)),
            Some(&NSString::from_str(&info.identifier)),
        )
    };
    if server.is_none() {
        tracing::error!("IMKServer 创建失败");
        std::process::exit(1);
    }
    let mtm = MainThreadMarker::new().expect("输入法入口必须在主线程");
    host::schedule_pause_timer(mtm);
    NSApplication::sharedApplication(mtm).run();
}
