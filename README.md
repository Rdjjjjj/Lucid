# Lucid

**不会的单词，先用拼音写上。**

Lucid 是一款 macOS 英文输入法。像平常一样写英文；遇到不会拼的单词，直接用拼音占位。句子写完后，AI 把它整理成通顺的英文，确认后再替换原文。

```text
I want to mai coffee.
→ I want to buy coffee.

juemingdushi is this movie good?
→ Is Breaking Bad a good show?
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

Lucid 不内置模型或 API Key，也不保存输入历史。API Key 存在 macOS Keychain 中，句子只发送到你配置的 AI 服务。

- **OpenAI 兼容**：例如 `https://api.example.com/v1`，使用 `Bearer` API Key。
- **Anthropic 兼容**：例如 `https://api.example.com`，使用 `x-api-key`。
- 远程服务必须使用 HTTPS；只有 `localhost`、`127.0.0.1` 和 `::1` 可以使用 HTTP。

## 系统要求

- macOS 13 (Ventura) 或更高版本
- Apple Silicon 或 Intel

## License

[MIT License](LICENSE)
