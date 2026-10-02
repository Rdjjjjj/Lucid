# Lucid

日常打字学英语的 macOS AI 输入法：不会的英文单词直接用拼音写，AI 把整句改写成语法通顺的英文，确认后再替换原文。

> Lucid 不内置任何模型或 API Key。用户需要在设置中填写自己的 AI 服务地址、模型和 API Key。

## 下载与安装

从 GitHub 的 [Releases](https://github.com/Rdjjjjj/Lucid/releases) 下载最新的 `Lucid-<版本>.pkg`：

1. 双击 `Lucid-<版本>.pkg` 并完成安装。
2. 注销并重新登录 macOS（输入法列表没有立即刷新时尤其需要）。
3. 打开「系统设置 → 键盘 → 文本输入 → 编辑」，添加并启用 **Lucid**。
4. 从「应用程序」打开 **Lucid**，填写 AI 服务地址和 API Key，点击「获取模型列表」，选择模型并保存。
5. 切换到 Lucid，在任意文本框输入内容；不会的英文单词直接用拼音写，输入完整句子后按标点或暂停约两秒，即可看到英文建议。

首次使用未签名公证版本时，macOS 可能显示安全提示；请在 Finder 中右键应用或安装包并选择「打开」。

## 系统要求

- macOS 13 (Ventura) 或更高版本
- Apple Silicon 或 Intel（发布脚本会生成 universal 二进制）

## AI 服务配置

- **OpenAI 兼容**：例如 `https://api.example.com/v1`，使用 `Bearer` API Key。
- **Anthropic 兼容**：例如 `https://api.example.com`，使用 `x-api-key`。
- 远程服务必须使用 HTTPS；仅允许 `localhost`、`127.0.0.1` 和 `::1` 使用 HTTP。

API Key 保存在 macOS Keychain 中；服务地址和模型保存在本机设置中。输入内容会发送到用户配置的 AI 服务商，项目不会内置共享 Key，也没有额外的统计上报服务。

## 从源码构建

需要安装 Xcode 和 macOS Command Line Tools：

```bash
swift test
./scripts/build-dmg.sh
```

产物输出到 `dist/`：

- `Lucid-<版本>.pkg`

默认版本从 `LucidApp/Info.plist` 读取，也可以临时覆盖：

```bash
VERSION=0.1.4 ./scripts/build-dmg.sh
```

发布新版本时：

1. 同步更新 `LucidApp/Info.plist` 和 `LucidInputMethod/Info.plist` 的版本号。
2. 提交并推送一个同版本的 Git 标签，例如 `v0.1.4`。
3. GitHub Actions 会自动测试、构建 universal `.pkg`，并创建 Release；也可以手动运行构建脚本后上传产物。
4. 如使用 Homebrew Cask，先从 GitHub Release 下载的最终 `.pkg` 计算 SHA-256，再更新 `homebrew/lucid.rb`；本地构建包和 GitHub Actions 构建包的 SHA-256 可能不同。

发布前可运行公开仓库检查：

```bash
./scripts/check-public-release.sh
```

它会检查待提交文件和现有 Git 历史中的常见密钥、私钥、证书/描述文件、本机绝对路径、版本不一致和构建产物。

## 签名说明

当前脚本使用免费的 ad-hoc 签名，适合个人分发和测试，但没有 Developer ID 签名与公证。若要让普通用户下载后几乎没有 Gatekeeper 提示，需要使用 Apple Developer 账号完成 Developer ID 签名和公证。

## License

本项目使用 [MIT License](LICENSE)。
