//! 原生设置窗口。Key 存在当前用户注册表，不进凭据管理器。

use std::sync::{Mutex, OnceLock};

use lucid_core::{AiConfiguration, AiProtocol};
use lucid_provider_http::HttpCorrectionService;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject, GetSysColorBrush},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CBS_DROPDOWN, CBS_DROPDOWNLIST, CW_USEDEFAULT, CreateWindowExW,
                DefWindowProcW, DispatchMessageW, ES_AUTOHSCROLL, ES_PASSWORD, GetClientRect,
                GetMessageW, GetWindowTextLengthW, GetWindowTextW, HMENU, MoveWindow,
                PostQuitMessage, RegisterClassExW, SendMessageW, SetWindowTextW, ShowWindow,
                TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_DESTROY,
                WM_SETFONT, WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW,
                WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{HSTRING, PCWSTR, w},
};

use crate::win32::{self, load_settings, save_settings};

const SAVE: usize = 1;
const FETCH: usize = 2;
const PROTOCOL: usize = 10;
const URL: usize = 11;
const MODEL: usize = 12;
const KEY: usize = 13;
const STATUS: usize = 14;

struct Controls {
    header: HWND,
    description: HWND,
    protocol_label: HWND,
    url_label: HWND,
    model_label: HWND,
    key_label: HWND,
    protocol: HWND,
    url: HWND,
    model: HWND,
    key: HWND,
    status: HWND,
    save: HWND,
    fetch: HWND,
}

fn controls() -> &'static Mutex<Option<Controls>> {
    static CONTROLS: OnceLock<Mutex<Option<Controls>>> = OnceLock::new();
    CONTROLS.get_or_init(|| Mutex::new(None))
}

unsafe impl Send for Controls {}
unsafe impl Sync for Controls {}

pub fn run() -> Result<(), String> {
    // Fixed-pixel Win32 layouts are otherwise scaled as a bitmap on Windows 11,
    // which makes Chinese text and child controls overlap at 125% or 150% DPI.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    win32::initialize_com().map_err(|error| format!("无法初始化 COM：{error}"))?;
    unsafe { create_window() }
}

unsafe fn create_window() -> Result<(), String> {
    let class_name = w!("LucidSettingsWindow");
    let instance = unsafe { GetModuleHandleW(None).map_err(|error| error.to_string())? };
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW
            | windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: HINSTANCE(instance.0),
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        lpszClassName: class_name,
        ..WNDCLASSEXW::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err("无法注册设置窗口。".to_owned());
    }
    let title = HSTRING::from("Lucid 设置");
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            560,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
        .map_err(|error| error.to_string())?
    };
    add_controls(window, HINSTANCE(instance.0))?;
    layout(window);
    load_existing();
    unsafe {
        let _ = ShowWindow(window, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
    }

    let mut message = windows::Win32::UI::WindowsAndMessaging::MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn add_controls(parent: HWND, instance: HINSTANCE) -> Result<(), String> {
    let plain = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0);
    let label_style = plain;
    let field_style =
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | ES_AUTOHSCROLL as u32);
    let combo_style =
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | CBS_DROPDOWNLIST as u32);
    let model_style = WINDOW_STYLE(
        WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | CBS_DROPDOWN as u32 | ES_AUTOHSCROLL as u32,
    );
    let button_style =
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32);

    let header = unsafe {
        child(
            parent,
            instance,
            "STATIC",
            "连接 AI 服务",
            0,
            0,
            0,
            0,
            20,
            plain,
            WINDOW_EX_STYLE::default(),
        )?
    };
    let description = unsafe {
        child(
            parent,
            instance,
            "STATIC",
            "输入你的服务地址和凭据，句子只会发送到这里。",
            0,
            0,
            0,
            0,
            21,
            plain,
            WINDOW_EX_STYLE::default(),
        )?
    };
    let protocol_label = unsafe {
        child(
            parent,
            instance,
            "STATIC",
            "接口协议",
            0,
            0,
            0,
            0,
            30,
            label_style,
            WINDOW_EX_STYLE::default(),
        )?
    };
    let url_label = unsafe {
        child(
            parent,
            instance,
            "STATIC",
            "服务地址",
            0,
            0,
            0,
            0,
            31,
            label_style,
            WINDOW_EX_STYLE::default(),
        )?
    };
    let model_label = unsafe {
        child(
            parent,
            instance,
            "STATIC",
            "模型",
            0,
            0,
            0,
            0,
            32,
            label_style,
            WINDOW_EX_STYLE::default(),
        )?
    };
    let key_label = unsafe {
        child(
            parent,
            instance,
            "STATIC",
            "API Key",
            0,
            0,
            0,
            0,
            33,
            label_style,
            WINDOW_EX_STYLE::default(),
        )?
    };

    let protocol = unsafe {
        child(
            parent,
            instance,
            "COMBOBOX",
            "",
            0,
            0,
            0,
            0,
            PROTOCOL,
            combo_style,
            WS_EX_CLIENTEDGE,
        )?
    };
    let url = unsafe {
        child(
            parent,
            instance,
            "EDIT",
            "",
            0,
            0,
            0,
            0,
            URL,
            field_style,
            WS_EX_CLIENTEDGE,
        )?
    };
    let model = unsafe {
        child(
            parent,
            instance,
            "COMBOBOX",
            "",
            0,
            0,
            0,
            0,
            MODEL,
            model_style,
            WS_EX_CLIENTEDGE,
        )?
    };
    let key_style = WINDOW_STYLE(field_style.0 | ES_PASSWORD as u32);
    let key = unsafe {
        child(
            parent,
            instance,
            "EDIT",
            "",
            0,
            0,
            0,
            0,
            KEY,
            key_style,
            WS_EX_CLIENTEDGE,
        )?
    };
    let status = unsafe {
        child(
            parent,
            instance,
            "STATIC",
            "填写自己的 AI 服务。句子只发给这个服务。",
            0,
            0,
            0,
            0,
            STATUS,
            label_style,
            WINDOW_EX_STYLE::default(),
        )?
    };
    let save = unsafe {
        child(
            parent,
            instance,
            "BUTTON",
            "保存设置",
            0,
            0,
            0,
            0,
            SAVE,
            button_style,
            WINDOW_EX_STYLE::default(),
        )?
    };
    let fetch = unsafe {
        child(
            parent,
            instance,
            "BUTTON",
            "获取模型",
            0,
            0,
            0,
            0,
            FETCH,
            WINDOW_STYLE(plain.0 | WS_TABSTOP.0),
            WINDOW_EX_STYLE::default(),
        )?
    };

    add_combo_item(protocol, "OpenAI 兼容");
    add_combo_item(protocol, "Anthropic 兼容");
    select_combo(protocol, 0);
    set_text_field(key, "");

    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    for control in [
        header,
        description,
        protocol_label,
        url_label,
        model_label,
        key_label,
        protocol,
        url,
        model,
        key,
        status,
        save,
        fetch,
    ] {
        unsafe {
            let _ = SendMessageW(
                control,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
    }

    if let Ok(mut slot) = controls().lock() {
        *slot = Some(Controls {
            header,
            description,
            protocol_label,
            url_label,
            model_label,
            key_label,
            protocol,
            url,
            model,
            key,
            status,
            save,
            fetch,
        });
    }
    Ok(())
}

fn layout(parent: HWND) {
    let Ok(slot) = controls().lock() else { return };
    let Some(controls) = slot.as_ref() else {
        return;
    };
    let mut rect = RECT::default();
    if unsafe { GetClientRect(parent, &mut rect) }.is_err() {
        return;
    }
    let width = (rect.right - rect.left).max(620);
    let field_x = 190;
    let field_width = (width - field_x - 32).max(300);
    let label_width = field_x - 48;
    let place = |window: HWND, x: i32, y: i32, w: i32, h: i32| unsafe {
        let _ = MoveWindow(window, x, y, w, h, true);
    };

    place(controls.header, 32, 26, width - 64, 30);
    place(controls.description, 32, 60, width - 64, 24);
    place(controls.protocol_label, 32, 116, label_width, 30);
    place(controls.protocol, field_x, 112, field_width, 32);
    place(controls.url_label, 32, 172, label_width, 30);
    place(controls.url, field_x, 168, field_width, 32);
    place(controls.model_label, 32, 228, label_width, 30);
    place(controls.model, field_x, 224, field_width, 32);
    place(controls.key_label, 32, 284, label_width, 30);
    place(controls.key, field_x, 280, field_width, 32);
    place(controls.status, 32, 344, width - 64, 48);
    place(controls.save, field_x, 422, 132, 36);
    place(controls.fetch, field_x + 148, 422, 132, 36);
}

unsafe fn child(
    parent: HWND,
    instance: HINSTANCE,
    class_name: &str,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
    style: WINDOW_STYLE,
    ex_style: WINDOW_EX_STYLE,
) -> Result<HWND, String> {
    let class_name = HSTRING::from(class_name);
    let text = HSTRING::from(text);
    unsafe {
        CreateWindowExW(
            ex_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(text.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            Some(parent),
            Some(HMENU(id as *mut core::ffi::c_void)),
            Some(instance),
            None,
        )
        .map_err(|error| error.to_string())
    }
}

fn load_existing() {
    let Ok(settings) = load_settings() else {
        set_status("无法读取已保存的设置。可直接填写后保存。");
        return;
    };
    let has_api_key = settings.api_key.is_some();
    let has_configuration = settings.configuration.is_some();
    let Ok(slot) = controls().lock() else { return };
    let Some(controls) = slot.as_ref() else {
        return;
    };
    if let Some(configuration) = settings.configuration {
        set_text(controls.url, &configuration.base_url);
        add_combo_item(controls.model, &configuration.model);
        select_combo(
            controls.protocol,
            match configuration.api_protocol {
                AiProtocol::OpenAiCompatible => 0,
                AiProtocol::AnthropicCompatible => 1,
            },
        );
    }
    drop(slot);
    if has_configuration {
        set_status("设置已加载。输入新 Key 可替换已保存的 Key。");
    } else if has_api_key {
        set_status("API Key 已保存。输入新 Key 可替换。");
    }
}

fn save_clicked() {
    let Ok(slot) = controls().lock() else { return };
    let Some(controls) = slot.as_ref() else {
        return;
    };
    let protocol_window = controls.protocol;
    let url_window = controls.url;
    let model_window = controls.model;
    let key_window = controls.key;
    drop(slot);

    let protocol = if combo_index(protocol_window) == 1 {
        AiProtocol::AnthropicCompatible
    } else {
        AiProtocol::OpenAiCompatible
    };
    let mut configuration =
        AiConfiguration::new(protocol, window_text(url_window), window_text(model_window));
    configuration.base_url = configuration.base_url.trim().to_owned();
    configuration.model = configuration.model.trim().to_owned();
    let mut settings = load_settings().unwrap_or(crate::settings::WindowsSettings {
        configuration: None,
        api_key: None,
    });
    settings.configuration = Some(configuration);
    let key = window_text(key_window);
    if !key.trim().is_empty() {
        settings.api_key = Some(key.trim().to_owned());
    }
    match save_settings(&settings) {
        Ok(()) => {
            set_text(key_window, "");
            set_status("设置已保存，切换到 Lucid 输入法后即可使用。");
        }
        Err(error) => set_status(&error),
    }
}

fn fetch_clicked() {
    fetch_models();
}

fn fetch_models() {
    save_clicked();
    let settings = match load_settings() {
        Ok(settings) => settings,
        Err(error) => {
            set_status(&error);
            return;
        }
    };
    let (Some(configuration), Some(api_key)) = (settings.configuration, settings.api_key) else {
        set_status("请先填写 API Key。");
        return;
    };
    let service = match HttpCorrectionService::new(configuration, api_key) {
        Ok(service) => service,
        Err(error) => {
            set_status(&error.to_string());
            return;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            set_status("无法发起模型请求。");
            return;
        }
    };
    match runtime.block_on(service.fetch_models()) {
        Ok(models) => {
            if let Ok(slot) = controls().lock()
                && let Some(controls) = slot.as_ref()
            {
                for model in &models {
                    add_combo_item(controls.model, model);
                }
            }
            set_status(&format!("已获取 {} 个模型。", models.len()));
        }
        Err(error) => set_status(&error.to_string()),
    }
}

fn set_status(text: &str) {
    if let Ok(slot) = controls().lock()
        && let Some(controls) = slot.as_ref()
    {
        set_text(controls.status, text);
    }
}

fn set_text(window: HWND, text: &str) {
    let text = HSTRING::from(text);
    unsafe {
        let _ = SetWindowTextW(window, PCWSTR(text.as_ptr()));
    }
}

fn set_text_field(window: HWND, text: &str) {
    set_text(window, text);
}

fn window_text(window: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

fn add_combo_item(window: HWND, text: &str) {
    const CB_ADDSTRING: u32 = 0x0143;
    let text = HSTRING::from(text);
    unsafe {
        let _ = SendMessageW(
            window,
            CB_ADDSTRING,
            None,
            Some(LPARAM(text.as_ptr() as isize)),
        );
    }
}

fn select_combo(window: HWND, index: usize) {
    const CB_SETCURSEL: u32 = 0x014E;
    unsafe {
        let _ = SendMessageW(window, CB_SETCURSEL, Some(WPARAM(index)), None);
    }
}

fn combo_index(window: HWND) -> isize {
    const CB_GETCURSEL: u32 = 0x0147;
    unsafe { SendMessageW(window, CB_GETCURSEL, None, None).0 }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            match wparam.0 & 0xffff {
                SAVE => save_clicked(),
                FETCH => fetch_clicked(),
                _ => {}
            }
            LRESULT(0)
        }
        WM_SIZE => {
            layout(window);
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
