//! TSF 到 LucidServer 的短连接客户端。
//!
//! 每次请求使用一个短连接，避免在 TSF 的 COM 线程里维护异步运行时。Server 端会
//! 用 session_id/request_id 丢弃旧结果，客户端在写回文档前还会进行第二次原文校验。

#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::io::{BufReader, BufWriter};

#[cfg(windows)]
use lucid_platform::{
    ClientMessage, PIPE_NAME, PROTOCOL_VERSION, ServerMessage, Utf16Range,
    protocol::{read_json, write_json},
};

#[cfg(windows)]
pub fn request_correction(
    session_id: u64,
    secure: bool,
    request_id: u64,
    text: &str,
    range: Utf16Range,
) -> Result<ServerMessage, String> {
    let stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_NAME)
        .map_err(|error| format!("LucidServer 不可用：{error}"))?;
    let reader_stream = stream
        .try_clone()
        .map_err(|error| format!("无法复制 LucidServer 管道：{error}"))?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = BufWriter::new(stream);
    write_json(
        &mut writer,
        &ClientMessage::OpenSession {
            session_id,
            secure,
            app: "tsf".to_owned(),
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .map_err(|error| error.to_string())?;
    write_json(
        &mut writer,
        &ClientMessage::TextObserved {
            session_id,
            request_id,
            text: text.to_owned(),
            range,
            timestamp_ms: crate::win32::monotonic_millis(),
        },
    )
    .map_err(|error| error.to_string())?;
    loop {
        let Some(message) =
            read_json::<_, ServerMessage>(&mut reader).map_err(|error| error.to_string())?
        else {
            return Err("LucidServer 已关闭连接。".to_owned());
        };
        match message {
            ServerMessage::CorrectionReady { .. }
            | ServerMessage::CorrectionFailed { .. }
            | ServerMessage::Error { .. } => return Ok(message),
            ServerMessage::SessionOpened { .. } | ServerMessage::ClearSuggestion { .. } => {}
        }
    }
}

#[cfg(windows)]
pub fn send_control(message: &ClientMessage) -> Result<(), String> {
    let session_id = match message {
        ClientMessage::FocusChanged { session_id, .. }
        | ClientMessage::AcceptSuggestion { session_id, .. }
        | ClientMessage::DismissSuggestion { session_id, .. } => *session_id,
        ClientMessage::OpenSession { session_id, .. }
        | ClientMessage::TextObserved { session_id, .. } => *session_id,
    };
    let stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_NAME)
        .map_err(|error| format!("LucidServer 不可用：{error}"))?;
    let reader_stream = stream
        .try_clone()
        .map_err(|error| format!("无法复制 LucidServer 管道：{error}"))?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = BufWriter::new(stream);
    write_json(
        &mut writer,
        &ClientMessage::OpenSession {
            session_id,
            secure: false,
            app: "tsf".to_owned(),
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .map_err(|error| error.to_string())?;
    write_json(&mut writer, message).map_err(|error| error.to_string())?;
    let _ = read_json::<_, ServerMessage>(&mut reader).map_err(|error| error.to_string())?;
    Ok(())
}
