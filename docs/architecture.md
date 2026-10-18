# Lucid 架构

> 本文记录已确认的目标架构。方案已进入实现阶段，本文记录最终边界和实现依据。

Lucid 采用“平台壳 + 共享 Core + Provider”的结构。这个结构参考了青简 REA 逆向报告中观察到的职责分离，但只复用工程方法，不复制青简的中文输入能力。

## 一句话判断标准

把 macOS InputMethodKit 换成 Windows TSF，`lucid-core` 的句子规则、请求过期规则和替换校验不应该改变。

## 当前仓库结构

```text
crates/lucid-core        句末状态机、文本范围模型、AI 请求模型
crates/lucid-platform    配置模型、跨平台 DTO、Windows IPC 帧
apps/macos               Rust InputMethodKit 壳、建议卡片、设置窗口
apps/windows/tsf         TSF/COM DLL、当前文档读写和范围校验
apps/windows/server      Named Pipe Server 和 Windows 请求入口
apps/windows/settings    设置窗口和 TSF 注册
apps/windows/installer   Windows 安装器
```

HTTP Provider 已从 `lucid-core` 独立出来；macOS API Key 与普通配置都保存在 Lucid 专用的用户默认值中。

## 输入路径

```text
系统输入事件
  → 平台壳立即插入普通文字
  → 当前 Session 更新句子和 text_version
  → 句末标点后创建 request_id
  → 后台 Provider 请求 AI
  → 校验 session_id / request_id / text_version
  → 显示一条建议
  → 用户确认后再次读回并替换准确范围
```

输入字符不等待网络。AI 失败、焦点变化、原文变化或请求过期时，原文保持不变。

## Core

`lucid-core` 是平台无关的唯一状态拥有者，负责：

- `Engine` 和每个输入上下文的 `InputSession`；
- 句末判断和待处理句子冻结；
- 文本版本和请求序号；
- 建议有效性与替换前置条件；
- `CorrectionProvider` 抽象接口。

Core 不负责：

- macOS/Windows API；
- 具体 UI；
- HTTP 请求细节；
- API Key 存储；
- 聊天软件的回车发送。

Core 不依赖 `reqwest`、Tokio、OpenAI 或 Anthropic。HTTP 实现位于 `lucid-provider-http`。

## Provider

Provider 将用户配置转换为具体服务商协议：

- OpenAI 兼容请求和响应；
- Anthropic 兼容请求和响应；
- 超时、网络和服务错误；
- 非标准响应解析和统一错误转换。

Provider 返回 Core 能理解的结构化建议，不直接操作文本，也不决定是否替换。

## macOS

macOS 平台壳使用 Rust 的 `objc2` 与 InputMethodKit 绑定：

- 接收输入法事件；
- 让普通字符立即进入当前应用；
- 维护宿主焦点和文本范围；
- 定位建议卡片；
- 在替换前读回并校验目标范围。

首版 macOS 输入法进程内运行 Core 和后台 Provider，不额外增加本地 Server。设置 App 是独立进程，只负责配置和安装引导，不参与日常输入。

## Windows

Windows TSF DLL 运行在宿主进程中，只做：

- TSF/COM 生命周期；
- 当前文档读写；
- UTF-16 范围和焦点校验；
- 与 Server 的协议通信。

`LucidServer.exe` 通过 Named Pipe 接收带有 `session_id`、`request_id`、`text_version` 的版本化 JSON 帧，负责 Core、Session 和 Provider。Server 不可用时，TSF 仍然放行普通输入。

`LucidSettings.exe` 负责当前用户配置、API Key 安全存储和 TSF 注册。安装器只负责复制文件和注册，不承载输入逻辑。

## 共享规则

- 英文立即上屏，拼音只是英文占位词；
- 句末标点触发 AI；
- AI 只提供建议，用户确认后替换；
- 用户继续输入或切换焦点后旧建议失效；
- 不自动发送聊天消息；
- 密码框和安全输入框不处理；
- 不保存输入历史。


## 已确认的运行规则

- 句末标点立即触发处理；无标点停顿约两秒也可以触发。
- 连续完成多个句子时，每句话独立处理。
- 网址、小数和缩写中的句号要进行基本判断，避免明显误触发。
- AI 请求不阻塞输入；完整结果返回后再显示建议。
- AI 报错时保留原文，并显示具体错误原因；重试使用当前最新原文。
- 用户移动光标后，旧建议失效；替换前再次核对输入框、原文、范围和请求版本。
- 无法可靠替换时保留原文并解释原因。
- 无法适配的聊天软件后续允许增加单独适配逻辑。


## 构建边界

Rust Cargo Workspace 是正式构建入口。Swift/Xcode 文件属于旧实现，迁移完成前可以保留用于历史参考，但不能由 CI、安装脚本或发布流程调用。

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo build -p lucid-macos --release
```

Windows 最终 TSF 注册、安装、输入行为和高 DPI 界面必须在 Windows 10/11 实机验收；macOS 交叉编译只能发现部分编译问题。
