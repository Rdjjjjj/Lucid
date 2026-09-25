# English Input

基于 AI 的 macOS 英文输入法：边输入边给出整句纠错建议，确认后替换。

## 安装

### 方式一：Homebrew（推荐）

```bash
brew install --cask rdj/tap/english-input
```

> 也可以直接从本仓库文件安装（前提是已发布对应版本的 Release）：
>
> ```bash
> brew install --cask ./homebrew/english-input.rb
> ```

### 方式二：手动下载

从 [Releases](https://github.com/rdj/EnglishInput/releases) 下载 `EnglishInput-<版本>.dmg`，打开后双击 `EnglishInput.pkg` 安装。

安装完成后：

1. 注销并重新登录。
2. 打开「系统设置 → 键盘 → 文本输入 → 编辑」，勾选并启用 **English Input**。
3. 从「应用程序」打开 **EnglishInput**，填写 AI 中转站地址、模型和 API Key。

## 系统要求

- macOS 13 (Ventura) 及以上
- Apple Silicon 或 Intel（通用二进制 universal）

## 构建

```bash
./scripts/build-dmg.sh
```

产物输出到 `dist/`：

- `EnglishInput.pkg`
- `EnglishInput-<版本>.dmg`

## 签名说明

本输入法采用 **ad-hoc 完整签名**（免费，无需 Apple Developer 账号），配合 Homebrew 分发可免去 Gatekeeper 拦截。

若想要「下载 dmg 双击即装、零警告」的体验，需要 Apple Developer 账号做 Developer ID 签名 + 公证。

## License

（请按需补充）
