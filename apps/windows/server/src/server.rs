use std::{
    collections::HashMap,
    ffi::OsStr,
    io,
    os::windows::ffi::OsStrExt,
    sync::{Arc, Mutex},
};

use lucid_core::CorrectionRequest;
use lucid_platform::{
    ClientMessage, MAX_FRAME_SIZE, PIPE_NAME, PROTOCOL_VERSION, ServerMessage, SharedSettings,
};
use lucid_provider_http::HttpCorrectionService;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions},
};
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::CreateMutexW,
};
use windows::core::PCWSTR;

const MUTEX_NAME: &str = "Local\\LucidServer.Singleton";

#[derive(Clone, Debug)]
struct Session {
    secure: bool,
    last_request_id: u64,
    active: bool,
}

#[derive(Default)]
struct ServerState {
    sessions: Mutex<HashMap<u64, Session>>,
}

pub fn run() -> Result<(), String> {
    let _mutex = Singleton::acquire()?;
    tracing_subscriber::fmt()
        .with_env_filter("lucid_windows_server=info")
        .try_init()
        .ok();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("无法创建 Server 运行时：{error}"))?;
    runtime.block_on(serve()).map_err(|error| error.to_string())
}

async fn serve() -> io::Result<()> {
    let state = Arc::new(ServerState::default());
    let mut first = true;
    loop {
        let mut options = ServerOptions::new();
        options
            .pipe_mode(PipeMode::Byte)
            .max_instances(255)
            .first_pipe_instance(first);
        let pipe = options.create(PIPE_NAME)?;
        first = false;
        pipe.connect().await?;
        let next_state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(error) = handle_client(pipe, next_state).await {
                tracing::debug!(%error, "Named Pipe 客户端结束");
            }
        });
    }
}

async fn handle_client(mut pipe: NamedPipeServer, state: Arc<ServerState>) -> Result<(), String> {
    let Some(first_message) = read_json(&mut pipe)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let ClientMessage::OpenSession {
        session_id,
        secure,
        protocol_version,
        ..
    } = first_message
    else {
        write_json(
            &mut pipe,
            &ServerMessage::Error {
                message: "连接必须先打开会话。".to_owned(),
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(());
    };
    if protocol_version != PROTOCOL_VERSION {
        write_json(
            &mut pipe,
            &ServerMessage::Error {
                message: format!(
                    "协议版本不匹配：客户端 {protocol_version}，Server {PROTOCOL_VERSION}。"
                ),
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(());
    }
    {
        let mut sessions = state
            .sessions
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        sessions
            .entry(session_id)
            .and_modify(|session| {
                session.secure = secure;
                session.active = true;
            })
            .or_insert(Session {
                secure,
                last_request_id: 0,
                active: true,
            });
    }
    write_json(
        &mut pipe,
        &ServerMessage::SessionOpened {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    loop {
        let Some(message) = read_json(&mut pipe)
            .await
            .map_err(|error| error.to_string())?
        else {
            break;
        };
        match message {
            ClientMessage::TextObserved {
                session_id: message_session,
                request_id,
                text,
                range,
                ..
            } => {
                if message_session != session_id || text.trim().is_empty() {
                    continue;
                }
                let allowed = {
                    let mut sessions = state
                        .sessions
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    match sessions.get_mut(&session_id) {
                        Some(session)
                            if !session.secure
                                && session.active
                                && request_id > session.last_request_id =>
                        {
                            session.last_request_id = request_id;
                            true
                        }
                        _ => false,
                    }
                };
                if !allowed {
                    continue;
                }
                let result = correct(text.clone()).await;
                match result {
                    Ok((replacement, chinese_text)) => {
                        let current = state
                            .sessions
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner())
                            .get(&session_id)
                            .is_some_and(|session| {
                                session.last_request_id == request_id && session.active
                            });
                        if current {
                            write_json(
                                &mut pipe,
                                &ServerMessage::CorrectionReady {
                                    session_id,
                                    request_id,
                                    original: text,
                                    replacement,
                                    range,
                                    chinese_text,
                                },
                            )
                            .await
                            .map_err(|error| error.to_string())?;
                        }
                    }
                    Err(message) => {
                        write_json(
                            &mut pipe,
                            &ServerMessage::CorrectionFailed {
                                session_id,
                                request_id,
                                message,
                            },
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                    }
                }
                break;
            }
            ClientMessage::FocusChanged {
                session_id: message_session,
                active,
            } => {
                if message_session == session_id {
                    if let Ok(mut sessions) = state.sessions.lock() {
                        if let Some(session) = sessions.get_mut(&session_id) {
                            session.active = active;
                            if !active {
                                session.last_request_id = session.last_request_id.wrapping_add(1);
                            }
                        }
                    }
                }
            }
            ClientMessage::AcceptSuggestion {
                session_id: message_session,
                request_id,
            }
            | ClientMessage::DismissSuggestion {
                session_id: message_session,
                request_id,
            } => {
                if message_session == session_id {
                    tracing::debug!(session_id, request_id, "建议状态已由 TSF 确认");
                }
                break;
            }
            ClientMessage::OpenSession { .. } => {}
        }
    }
    // 会话跨越短连接保留，避免旧请求在新连接建立后重新从 request_id=0 开始。
    let _ = pipe.disconnect();
    Ok(())
}

async fn correct(text: String) -> Result<(String, Option<String>), String> {
    let settings = load_settings()?;
    let Some(ref api_key) = settings.api_key else {
        return Err("请先在 LucidSettings.exe 中填写 API Key。".to_owned());
    };
    let configuration = settings.configuration();
    let service =
        HttpCorrectionService::new(configuration, api_key).map_err(|error| error.to_string())?;
    let result = service
        .correct(&CorrectionRequest::new(text))
        .await
        .map_err(|error| error.to_string())?;
    if result.corrected_text.trim().is_empty() {
        return Err("AI 返回了空的改写结果。".to_owned());
    }
    Ok((result.corrected_text, result.chinese_text))
}

fn load_settings() -> Result<SharedSettings, String> {
    let configuration = read_registry_string(lucid_platform::CONFIGURATION_KEY)?;
    let api_key = read_registry_string(lucid_platform::API_KEY_KEY)?;
    SharedSettings::from_values(configuration.as_deref(), api_key.as_deref())
}

fn read_registry_string(name: &str) -> Result<Option<String>, String> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let path = wide(r"Software\Lucid");
    let value = wide(name);
    let mut bytes = vec![0u8; 8192];
    let mut size = bytes.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.0 == 2 {
        return Ok(None);
    }
    if status.0 != 0 {
        return Err(format!("无法读取 Lucid 设置 {name}。"));
    }
    let units = (size as usize / 2).min(bytes.len() / 2);
    let wide = unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u16>(), units) };
    Ok(Some(
        String::from_utf16_lossy(wide)
            .trim_end_matches('\0')
            .to_owned(),
    ))
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

struct Singleton(HANDLE);

impl Singleton {
    fn acquire() -> Result<Self, String> {
        let name = wide(MUTEX_NAME);
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .map_err(|error| format!("无法创建 Server 单实例锁：{error}"))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("LucidServer 已经在运行。".to_owned());
        }
        Ok(Self(handle))
    }
}

impl Drop for Singleton {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

async fn read_json<R, T>(reader: &mut R) -> io::Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: for<'de> serde::Deserialize<'de>,
{
    let mut length_bytes = [0u8; 4];
    match reader.read_exact(&mut length_bytes).await {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_FRAME_SIZE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "IPC 帧超过上限"));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

async fn write_json<W, T>(writer: &mut W, value: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: serde::Serialize,
{
    let body = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if body.len() > MAX_FRAME_SIZE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "IPC 帧超过上限"));
    }
    writer.write_all(&(body.len() as u32).to_le_bytes()).await?;
    writer.write_all(&body).await?;
    writer.flush().await
}
