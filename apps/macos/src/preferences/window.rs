//! 设置窗口。保存后输入法从同一份用户默认值读取，不需要重启电脑。

use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSPopUpButton,
    NSSecureTextField, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use lucid_core::{AiConfiguration, AiProtocol};

use crate::app::settings::Settings;

pub fn run() {
    let mtm = MainThreadMarker::new().expect("设置窗口必须在主线程");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // A settings app is often launched while System Settings or Finder is
    // frontmost. Explicit activation is required for the newly created window
    // to become visible rather than leaving a background process behind.
    app.activateIgnoringOtherApps(true);
    let _controller = SettingsController::new(mtm);
    app.run();
}

struct SettingsIvars {
    window: Retained<NSWindow>,
    protocol_popup: Retained<NSPopUpButton>,
    url_field: Retained<NSTextField>,
    model_popup: Retained<NSPopUpButton>,
    key_field: Retained<NSSecureTextField>,
    status: Retained<NSTextField>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LucidSettingsController"]
    #[ivars = SettingsIvars]
    struct SettingsController;

    impl SettingsController {
        #[unsafe(method(save:))]
        fn save(&self, _sender: Option<&NSObject>) {
            self.save_settings();
        }

        #[unsafe(method(fetchModels:))]
        fn fetch_models(&self, _sender: Option<&NSObject>) {
            self.load_model_list();
        }
    }

    unsafe impl NSObjectProtocol for SettingsController {}
);

impl SettingsController {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(220.0, 180.0), NSSize::new(680.0, 460.0)),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("Lucid"));
        let content = window.contentView().expect("content");
        let protocol_popup = NSPopUpButton::new(mtm);
        protocol_popup.addItemWithTitle(&NSString::from_str("OpenAI 兼容"));
        protocol_popup.addItemWithTitle(&NSString::from_str("Anthropic 兼容"));
        protocol_popup.setFrame(NSRect::new(
            NSPoint::new(180.0, 390.0),
            NSSize::new(280.0, 28.0),
        ));
        let url_field = text_field(
            mtm,
            "https://api.example.com",
            NSPoint::new(180.0, 340.0),
            false,
        );
        let model_popup = NSPopUpButton::new(mtm);
        model_popup.addItemWithTitle(&NSString::from_str("请先获取模型列表"));
        model_popup.setFrame(NSRect::new(
            NSPoint::new(180.0, 290.0),
            NSSize::new(430.0, 28.0),
        ));
        let key_field = secure_field(mtm, NSPoint::new(180.0, 240.0));
        let status = NSTextField::labelWithString(
            &NSString::from_str("填写自己的 AI 服务。句子只发给这个服务。"),
            mtm,
        );
        status.setFrame(NSRect::new(
            NSPoint::new(36.0, 36.0),
            NSSize::new(600.0, 44.0),
        ));
        add_label(&content, mtm, "接口协议", NSPoint::new(36.0, 394.0));
        add_label(&content, mtm, "服务地址", NSPoint::new(36.0, 344.0));
        add_label(&content, mtm, "模型", NSPoint::new(36.0, 294.0));
        add_label(&content, mtm, "API Key", NSPoint::new(36.0, 244.0));
        content.addSubview(&protocol_popup);
        content.addSubview(&url_field);
        content.addSubview(&model_popup);
        content.addSubview(&key_field);
        content.addSubview(&status);

        let this = Self::alloc(mtm).set_ivars(SettingsIvars {
            window: window.clone(),
            protocol_popup: protocol_popup.clone(),
            url_field: url_field.clone(),
            model_popup: model_popup.clone(),
            key_field: key_field.clone(),
            status: status.clone(),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let save = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("保存设置"),
                Some(&this),
                Some(sel!(save:)),
                mtm,
            )
        };
        save.setFrame(NSRect::new(
            NSPoint::new(180.0, 180.0),
            NSSize::new(120.0, 32.0),
        ));
        let fetch = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("获取模型"),
                Some(&this),
                Some(sel!(fetchModels:)),
                mtm,
            )
        };
        fetch.setFrame(NSRect::new(
            NSPoint::new(312.0, 180.0),
            NSSize::new(120.0, 32.0),
        ));
        content.addSubview(&save);
        content.addSubview(&fetch);
        this.load_existing();
        window.center();
        window.makeKeyAndOrderFront(None);
        // Do not perform a synchronous network request while constructing the
        // window.  The old automatic model fetch blocked the main thread before
        // NSApplication.run(), which made the app look as if it could not open.
        // The user can fetch models after the window is visible.
        this
    }

    fn load_existing(&self) {
        let settings = Settings::shared();
        if let Some(configuration) = settings.configuration() {
            self.ivars()
                .url_field
                .setStringValue(&NSString::from_str(&configuration.base_url));
            self.fill_models(&[configuration.model.clone()], &configuration.model);
            let index = match configuration.api_protocol {
                AiProtocol::OpenAiCompatible => 0,
                AiProtocol::AnthropicCompatible => 1,
            };
            self.ivars().protocol_popup.selectItemAtIndex(index);
        }
        if settings.api_key().is_some() {
            self.ivars()
                .status
                .setStringValue(&NSString::from_str("API Key 已保存。输入新 Key 可替换。"));
        }
    }

    fn save_settings(&self) {
        let ivars = self.ivars();
        let protocol = if ivars.protocol_popup.indexOfSelectedItem() == 1 {
            AiProtocol::AnthropicCompatible
        } else {
            AiProtocol::OpenAiCompatible
        };
        let mut configuration = AiConfiguration::new(
            protocol,
            ivars.url_field.stringValue().to_string(),
            self.selected_model(),
        );
        configuration.base_url = configuration.base_url.trim().to_owned();
        configuration.model = configuration.model.trim().to_owned();
        let settings = Settings::shared();
        if let Err(error) = settings.save_configuration(&configuration) {
            ivars.status.setStringValue(&NSString::from_str(&error));
            return;
        }
        let key = ivars.key_field.stringValue().to_string();
        if !key.trim().is_empty() {
            settings.save_api_key(key.trim());
            ivars.key_field.setStringValue(&NSString::from_str(""));
        }
        ivars
            .status
            .setStringValue(&NSString::from_str("设置已保存。切到 Lucid 后即可使用。"));
    }

    fn load_model_list(&self) {
        self.save_settings();
        let settings = Settings::shared();
        let Some(configuration) = settings.configuration() else {
            return;
        };
        let Some(api_key) = settings.api_key() else {
            self.ivars()
                .status
                .setStringValue(&NSString::from_str("请先填写 API Key。"));
            return;
        };
        let service = match lucid_core::ai::HttpCorrectionService::new(configuration, api_key) {
            Ok(service) => service,
            Err(error) => {
                self.ivars()
                    .status
                    .setStringValue(&NSString::from_str(&error.to_string()));
                return;
            }
        };
        self.ivars()
            .status
            .setStringValue(&NSString::from_str("正在获取模型列表…"));
        match std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok()?;
                runtime.block_on(service.fetch_models()).ok()
            });
            handle.join().ok().flatten()
        }) {
            Some(models) if !models.is_empty() => {
                let saved = self.selected_model();
                self.fill_models(&models, &saved);
                let selected = self.selected_model();
                self.ivars()
                    .status
                    .setStringValue(&NSString::from_str(&format!(
                        "已获取 {} 个模型，从下拉框选择。当前：{}",
                        models.len(),
                        selected
                    )));
                let mut configuration = self.current_configuration();
                configuration.model = selected;
                let _ = Settings::shared().save_configuration(&configuration);
            }
            _ => self
                .ivars()
                .status
                .setStringValue(&NSString::from_str("获取模型列表失败，请检查地址和 Key。")),
        }
    }

    fn current_configuration(&self) -> AiConfiguration {
        let ivars = self.ivars();
        let protocol = if ivars.protocol_popup.indexOfSelectedItem() == 1 {
            AiProtocol::AnthropicCompatible
        } else {
            AiProtocol::OpenAiCompatible
        };
        AiConfiguration::new(
            protocol,
            ivars.url_field.stringValue().to_string(),
            self.selected_model(),
        )
    }

    fn selected_model(&self) -> String {
        self.ivars()
            .model_popup
            .titleOfSelectedItem()
            .map(|title| title.to_string())
            .filter(|title| !title.is_empty() && title != "请先获取模型列表")
            .unwrap_or_default()
    }

    fn fill_models(&self, models: &[String], preferred: &str) {
        let popup = &self.ivars().model_popup;
        popup.removeAllItems();
        let mut names = models.to_vec();
        names.retain(|name| !name.trim().is_empty());
        names.sort();
        names.dedup();
        if names.is_empty() {
            popup.addItemWithTitle(&NSString::from_str("请先获取模型列表"));
            return;
        }
        for name in &names {
            popup.addItemWithTitle(&NSString::from_str(name));
        }
        let chosen = choose_model(&names, preferred);
        popup.selectItemWithTitle(&NSString::from_str(&chosen));
    }
}

fn text_field(
    mtm: MainThreadMarker,
    placeholder: &str,
    origin: NSPoint,
    _secure: bool,
) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
    field.setFrame(NSRect::new(origin, NSSize::new(430.0, 28.0)));
    field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
    field
}

fn secure_field(mtm: MainThreadMarker, origin: NSPoint) -> Retained<NSSecureTextField> {
    let field = NSSecureTextField::new(mtm);
    field.setFrame(NSRect::new(origin, NSSize::new(430.0, 28.0)));
    field.setPlaceholderString(Some(&NSString::from_str("粘贴 API Key")));
    field
}

fn add_label(content: &NSView, mtm: MainThreadMarker, text: &str, origin: NSPoint) {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(NSRect::new(origin, NSSize::new(120.0, 22.0)));
    content.addSubview(&label);
}

fn choose_model(models: &[String], saved: &str) -> String {
    if !saved.is_empty() && models.iter().any(|name| name == saved) && !is_auxiliary_model(saved) {
        return saved.to_owned();
    }
    for hint in [
        "deepseek-flash",
        "deepseek-chat",
        "deepseek",
        "gpt-4",
        "claude",
        "qwen",
        "gemini",
        "glm",
    ] {
        if let Some(found) = models
            .iter()
            .find(|name| name.to_ascii_lowercase().contains(hint) && !is_auxiliary_model(name))
        {
            return found.clone();
        }
    }
    models
        .iter()
        .find(|name| !is_auxiliary_model(name))
        .cloned()
        .unwrap_or_else(|| models[0].clone())
}

fn is_auxiliary_model(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "bge",
        "embed",
        "rerank",
        "whisper",
        "tts",
        "dall-e",
        "moderation",
    ]
    .iter()
    .any(|token| lower.contains(token))
}
