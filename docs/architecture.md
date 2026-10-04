# 架构

Lucid 按青简的语言架构拆开：Core 不认识 macOS，平台壳不改句子。

```text
crates/lucid-core        句末状态机、范围恢复、替换校验、AI 协议
crates/lucid-platform    主 App 与输入法共用的配置键
apps/macos               InputMethodKit 壳、建议卡片、设置窗口
```

判断标准：把 IMK 换成别的输入法框架，不应该改 `lucid-core` 的任何一行。

## 输入路径

```text
键盘事件
  → 立刻插入普通文字
  → 更新当前会话的句子状态机
  → 标点或停顿两秒后异步请求用户配置的 AI
  → 校验焦点和原句仍然有效
  → 显示建议卡片
  → 用户确认后才替换已输入句子
```

拼音只是英文句子里的占位词。Lucid 不提供中文候选，也不在未确认时替换或发送。
