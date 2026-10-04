//! 日志只记按键来源和结果状态，不记句子正文，也不记 API Key。

pub struct Guard;

pub fn init() -> Guard {
    let path = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Library/Logs/Lucid/lucid.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || file.try_clone().expect("log file"))
            .try_init();
    }
    Guard
}
