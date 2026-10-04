//! InputMethodKit 这一侧。
//!
//! 只把系统按键交给会话，把 Core 给出的建议交给面板。句末判断、协议解析和替换校验不在这里。

mod client;
mod controller;
mod session;

pub use client::{TextClient, is_authentication_host, secure_input_enabled};
pub use controller::{LucidInputController, verify_event_entries};
pub use session::InputSession;
