//! LucidServer.exe：唯一持有 AI 请求和 Windows IPC 会话的进程。
//!
//! TSF DLL 不读取配置、不创建 HTTP 客户端。它只把已经写入文档的句子和 UTF-16
//! 范围发到这里，结果回来后仍由 TSF 侧进行原文校验。

#[cfg(windows)]
mod server;

#[cfg(windows)]
fn main() {
    if let Err(error) = server::run() {
        eprintln!("LucidServer 启动失败：{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LucidServer 只能在 Windows 上运行。");
}
