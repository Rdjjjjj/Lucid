# Lucid 技术栈决策

- 版本：0.3
- 日期：2026-10-17
- 状态：技术栈方案已对齐，Provider 与 macOS 配置已完成首轮实现
- 参考：[青简 v0.2.1 REA 逆向分析](research/qingjian-rea/README.md)

## 1. 结论

Lucid 继续使用 Rust。需要调整的是工程边界，不是产品核心：

- Rust 2024 是唯一正式开发语言；
- Cargo Workspace 是唯一正式构建入口；
- macOS 和 Windows 的系统输入法接口保留在各自平台层；
- 输入行为、Session 和请求有效性放进共享 Rust Core；
- HTTP 请求和服务商协议放进 Provider 层；
- Windows TSF DLL 与 AI Server 分进程；
- Swift/Xcode 旧体系退出正式构建和发布流程。

青简值得借鉴的是平台壳与 Engine 分离、Session 管理、本地输入路径不等待网络、旧结果淘汰和替换前校验。Lucid 不复制青简的中文词库、中文候选、拼音模型和中文输入状态机。

## 2. 正式技术栈

| 用途 | 技术 | 边界 |
| --- | --- | --- |
| 主语言 | Rust 2024 | 所有正式产品组件 |
| 工程管理 | Cargo Workspace | 依赖、版本、测试、发布 |
| 核心模型 | Rust + Serde | Core 和协议数据 |
| 错误处理 | `thiserror` | 各层错误转换 |
| 日志 | `tracing` | 只记录元数据 |
| 异步 | Tokio | Provider 外层、Windows Server |
| HTTP | `reqwest` | 仅 `lucid-provider-http` |
| macOS 桥接 | `objc2` + InputMethodKit | macOS 平台壳 |
| macOS 原生 UI | `objc2-app-kit` | 设置窗口和建议卡片 |
| Windows 平台 | TSF / COM + `windows` crate | TSF DLL 和设置 |
| 本地通信 | Windows Named Pipe + JSON 帧 | TSF 与 Server |
| macOS 配置 | 用户默认值（UserDefaults） | API Key 和普通配置 |
| Windows 密钥 | DPAPI 或 Credential Manager | API Key |

## 3. 目标工程结构

```text
crates/
├── lucid-core
│   ├── Engine
│   ├── InputSession
│   ├── SentenceTracker
│   ├── RequestScheduler
│   ├── CorrectionProvider 接口
│   ├── Suggestion 模型
│   └── ReplacementValidator
├── lucid-platform
│   ├── 配置模型
│   └── 跨平台 DTO / Windows IPC 帧
└── lucid-provider-http
    ├── reqwest
    ├── OpenAI 兼容适配器
    ├── Anthropic 兼容适配器
    └── 响应解析和错误转换

apps/
├── macos
│   ├── InputMethodKit 平台壳
│   ├── 建议卡片
│   └── 设置 App
└── windows
    ├── tsf       TSF/COM DLL
    ├── server    Core、Session、Provider 和 Named Pipe
    ├── settings  设置和 TSF 注册
    └── installer 安装程序
```

## 4. 依赖规则

### `lucid-core`

Core 只处理句末判断、Session、版本、请求生命周期、建议状态和替换前置条件。

Core 不依赖 macOS、Windows、UI、`reqwest`、Tokio、具体 AI 服务商或 API Key 存储。

### `lucid-provider-http`

Provider 负责 HTTP、OpenAI/Anthropic 请求格式、响应解析、超时和网络错误。它通过 Core 定义的 `CorrectionProvider` 接口提供服务，不操作文本，不决定替换。

### 平台层

平台层负责系统事件、当前文本、焦点、范围、建议展示和替换执行。平台层不能重新实现句末判断、请求过期判断或 AI 结果校验。

## 5. 进程边界

### macOS

首版不额外增加本地 Server：

```text
InputMethodKit → lucid-core → 后台 Provider → 建议卡片
```

输入法进程必须保持输入事件快速返回；设置 App 只是配置界面，不是输入法的依赖服务。后续只有在需要崩溃隔离、统一请求管理或多输入法实例共享时，才评估增加本地 Helper。

### Windows

```text
宿主应用
  └── LucidTextService.dll
          │ Named Pipe
          ▼
      LucidServer.exe
        ├── lucid-core
        ├── Session
        └── lucid-provider-http
```

TSF DLL 不直接访问网络。Server 不可用时，TSF 仍放行普通输入。

## 6. Session 和请求有效性

每个输入上下文独立管理：

- `session_id`：输入框生命周期；
- `text_version`：原文变化版本；
- `request_id`：当前请求序号；
- 当前焦点、句子范围和建议状态。

AI 返回必须匹配：

```text
session_id + request_id + text_version
```

用户确认替换时，平台层还要重新确认焦点、范围和原文。任一条件不满足就丢弃建议，不能删除后重新输入，也不能把结果插入新窗口。

## 7. 配置和安全

macOS 的 API Key 和普通配置都存储在 Lucid 专用的用户默认值中，输入法读取时不会触发钥匙串授权弹窗。Windows 后续使用当前用户配置存储；正式分发时再单独评估系统安全存储。

输入句子只发送当前待处理句子，不保存输入历史。日志只记录请求元数据、耗时和错误类别，不记录正文、按键内容和 API Key。密码框和安全输入框不创建 Session。

## 8. 构建和发布

正式 CI 和发布应执行：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo build -p lucid-macos --release
```

首版 macOS 使用 `.pkg` 作为主要安装格式，负责安装到系统输入法目录、注册输入源和处理升级。签名策略暂时参考青简：App 使用 ad-hoc 签名，安装包不做 Developer ID 签名和 Apple 公证。这个选择适合当前验证阶段，但用户安装时可能需要手动允许 macOS 安全提示；面向公开分发时再评估正式签名和公证。

macOS bundle、安装和 PKG 脚本必须使用 `target/release` 的 Rust 产物。Windows 使用各自 Cargo package 构建。`xcodebuild`、`swift test` 和 Swift Package 不再属于正式 CI；Swift/Xcode 文件在迁移完成前标记为 legacy。

## 9. 已确认的技术选择

本轮确认结果全部采用推荐项：Rust 2024 作为唯一正式语言；采用平台壳、共享 Core 和 Provider 分层；macOS 使用 Rust + InputMethodKit，首版不增加本地 Server；Windows 使用 TSF DLL + Named Pipe + Server；输入和 AI 请求完全异步；每个输入框独立管理状态；旧请求结果直接丢弃；替换前重新核对输入框、原文、范围和请求版本；支持 OpenAI 兼容与 Anthropic 兼容服务；API Key 和普通配置使用 Lucid 专用的 macOS 用户默认值；只发送当前句子；不保存输入历史；Cargo Workspace 作为正式构建入口；Swift/Xcode 最终归档或移除；先完成 macOS 和聊天软件验收，再做 Windows。

## 10. 收敛顺序

1. 文档统一为 Rust 正式方案。
2. CI、安装、打包脚本统一为 Cargo 入口。
3. 新增 `lucid-provider-http`，移出 Core 的网络依赖。
4. 建立 `Engine`、`InputSession`、`RequestScheduler` 和统一版本校验。
5. 完成 macOS 配置存储和 Windows 后续配置方案。
6. 完成 macOS 跨应用和聊天软件验收。
7. Rust 闭环稳定后归档或移除 Swift/Xcode 旧实现。
