//! 各平台壳共用的配置、协议 DTO 和 IPC 帧编解码。

pub mod config;
pub mod protocol;

pub use config::{API_KEY_KEY, CONFIGURATION_KEY, DEFAULTS_SUITE, SharedSettings};
pub use protocol::{
    ClientMessage, FrameError, MAX_FRAME_SIZE, PIPE_NAME, PROTOCOL_VERSION, ServerMessage,
    Utf16Range,
};
