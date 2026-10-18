//! Lucid Windows 壳之间共享的进程间协议。
//!
//! 消息体保持平台无关，Named Pipe 只是传输层。每个帧使用四字节小端长度前缀，
//! 后跟一个 UTF-8 JSON 对象，避免 TSF DLL 和 Server 依赖相同的异步运行时。

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const PIPE_NAME: &str = r"\\.\pipe\lucid";
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    OpenSession {
        session_id: u64,
        secure: bool,
        app: String,
        protocol_version: u32,
    },
    TextObserved {
        session_id: u64,
        request_id: u64,
        text: String,
        range: Utf16Range,
        timestamp_ms: u64,
    },
    FocusChanged {
        session_id: u64,
        active: bool,
    },
    AcceptSuggestion {
        session_id: u64,
        request_id: u64,
    },
    DismissSuggestion {
        session_id: u64,
        request_id: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    SessionOpened {
        protocol_version: u32,
    },
    CorrectionReady {
        session_id: u64,
        request_id: u64,
        original: String,
        replacement: String,
        range: Utf16Range,
        chinese_text: Option<String>,
    },
    CorrectionFailed {
        session_id: u64,
        request_id: u64,
        message: String,
    },
    ClearSuggestion {
        session_id: u64,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Utf16Range {
    pub location: usize,
    pub length: usize,
}

impl Utf16Range {
    pub const fn new(location: usize, length: usize) -> Self {
        Self { location, length }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("读取帧长度失败：{0}")]
    ReadLength(#[source] io::Error),
    #[error("读取帧内容失败：{0}")]
    ReadBody(#[source] io::Error),
    #[error("写入帧失败：{0}")]
    Write(#[source] io::Error),
    #[error("帧长度 {0} 超出上限 {MAX_FRAME_SIZE}")]
    TooLarge(usize),
    #[error("JSON 帧无效：{0}")]
    Json(#[source] serde_json::Error),
}

pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>, FrameError> {
    let mut length_bytes = [0u8; 4];
    match reader.read_exact(&mut length_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(FrameError::ReadLength(error)),
    }
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge(length));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(FrameError::ReadBody)?;
    Ok(Some(body))
}

pub fn write_frame<W: Write>(writer: &mut W, body: &[u8]) -> Result<(), FrameError> {
    if body.len() > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge(body.len()));
    }
    let length = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(body.len()))?;
    writer
        .write_all(&length.to_le_bytes())
        .and_then(|_| writer.write_all(body))
        .and_then(|_| writer.flush())
        .map_err(FrameError::Write)
}

pub fn read_json<R: Read, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> Result<Option<T>, FrameError> {
    let Some(body) = read_frame(reader)? else {
        return Ok(None);
    };
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(FrameError::Json)
}

pub fn write_json<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), FrameError> {
    let body = serde_json::to_vec(value).map_err(FrameError::Json)?;
    write_frame(writer, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_json_with_a_little_endian_length_prefix() {
        let message = ClientMessage::TextObserved {
            session_id: 7,
            request_id: 9,
            text: "hello".to_owned(),
            range: Utf16Range::new(3, 5),
            timestamp_ms: 11,
        };
        let mut bytes = Vec::new();
        write_json(&mut bytes, &message).unwrap();
        let expected = serde_json::to_vec(&message).unwrap();
        assert_eq!(&bytes[..4], &(expected.len() as u32).to_le_bytes());
        assert_eq!(
            read_json::<_, ClientMessage>(&mut Cursor::new(bytes)).unwrap(),
            Some(message)
        );
    }

    #[test]
    fn eof_before_a_new_frame_is_clean_shutdown() {
        assert_eq!(
            read_frame(&mut Cursor::new(Vec::<u8>::new())).unwrap(),
            None
        );
    }

    #[test]
    fn incomplete_body_is_an_error() {
        let mut bytes = (3u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(b"ab");
        assert!(matches!(
            read_frame(&mut Cursor::new(bytes)),
            Err(FrameError::ReadBody(_))
        ));
    }

    #[test]
    fn oversized_frames_are_rejected_before_allocation() {
        let bytes = ((MAX_FRAME_SIZE as u32) + 1).to_le_bytes();
        assert!(matches!(
            read_frame(&mut Cursor::new(bytes)),
            Err(FrameError::TooLarge(_))
        ));
        assert!(matches!(
            write_frame(&mut Vec::new(), &vec![0; MAX_FRAME_SIZE + 1]),
            Err(FrameError::TooLarge(_))
        ));
    }
}
