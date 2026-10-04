# Lucid

**不会的单词，先用拼音写上。**

Lucid 是一款 macOS 英文输入法。像平常一样写英文；遇到不会拼的单词，直接用拼音占位。句子写完后，AI 把它整理成通顺的英文，确认后再替换原文。

```text
I want to mai coffee.
→ I want to buy coffee.

this coffee tai haohe le.
→ This coffee tastes great.
```

拼音只是暂时占位，不会打断正在输入的句子。Lucid 只提供建议，不会自动改写或发送。

## 下载

从 [Releases](https://github.com/Rdjjjjj/Lucid/releases) 下载最新的 `Lucid-<版本>.pkg`，双击安装。

安装后如果输入法列表没有 Lucid，先注销并重新登录，再到「系统设置 → 键盘 → 文本输入 → 编辑」添加并启用。然后打开「应用程序」中的 Lucid，填写自己的 AI 服务地址、模型和 API Key。

首次打开未签名公证的安装包时，macOS 可能显示安全提示；在 Finder 中右键安装包并选择「打开」。

## 怎么用

1. 切换到 Lucid，在任意文本框里输入英文。
2. 不会拼的单词直接写拼音，例如 `mai`、`juemingdushi`。
3. 输入句号、问号、感叹号，或停顿约两秒。
4. 查看建议后，选择使用英文或保留原文。

Lucid 不内置模型或 API Key，也不保存输入历史。API Key 只存在 Lucid 自己的本机设置里，不会写入钥匙串；句子只发送到你配置的 AI 服务。

- **OpenAI 兼容**：例如 `https://api.example.com/v1`，使用 `Bearer` API Key。
- **Anthropic 兼容**：例如 `https://api.example.com`，使用 `x-api-key`。
- 远程服务必须使用 HTTPS；只有 `localhost`、`127.0.0.1` 和 `::1` 可以使用 HTTP。

## 系统要求

- macOS 13 (Ventura) 或更高版本
- Apple Silicon 或 Intel

## License

[MIT License](LICENSE)

## 开发构建

当前 macOS 版本用 Rust 重写，产品行为不变：英文直接输入，不会拼的词用拼音占位，句子结束后给出建议，确认后才替换。

```bash
apps/macos/scripts/bundle.sh
apps/macos/scripts/install-local.sh
```

安装后如果输入法列表没有 Lucid，注销并重新登录，再到「系统设置 → 键盘 → 文本输入」添加。设置窗口在「应用程序」里的 Lucid。

### 安装与排查（Rust 0.2.9）

- Lucid 只注册一个可选择输入源，不再创建默认开启的 `.english` 子模式。移除时不会留下仍开启的父模式；已启用的输入源也不会重复启用。
- 旧版 App 备份为 `.tar.gz`，而非简单改名为 `.app.saved`；后者仍可能被系统索引为输入法。

- 本地安装只更新 `/Library/Input Methods/LucidInputMethod.app` 和 `/Applications/Lucid.app`，不再在系统安装失败时静默创建用户目录副本。
- 输入法与设置 App 来自同一构建；安装前检查输入回调的 Objective-C ABI，安装后逐字节核对两个签名后的安装产物。
- 可分发安装包用 `apps/macos/scripts/package.sh` 生成，包禁止 Installer 自动迁移到旧的构建目录。已有正式安装可用 `--installed` 打包，不再生成临时输入源。
- 输入回调和翻译请求状态记录在 `~/Library/Logs/Lucid/lucid.log`，不记录正文、按键编码或 API Key；密码/认证输入不跟踪、不翻译。
- 更新后请在目标应用新建空白输入框，重新选择 Lucid，**手动**输入 `nihao.` 做验收。直接粘贴或发给后台应用的自动化输入，不能验证 macOS 的 InputMethodKit 分发链路。
