Lucid 0.2.12 Windows 安装包
目标：Windows 10/11 x64

安装：
1. 双击 LucidSetup.exe。
2. 如果 Windows 显示“未知发布者”，选择“仍要运行”。
3. 安装程序会自动复制组件、注册 Lucid 输入法、设置后台服务开机启动并打开设置窗口。
4. 在设置窗口填写 AI 服务地址、模型和 API Key。
5. 到“设置 -> 时间和语言 -> 输入 -> 添加 Lucid”启用输入法。

卸载：
在本目录打开 PowerShell，执行：
   powershell -ExecutionPolicy Bypass -File .\uninstall.ps1

LucidSetup.exe 是单文件安装程序，已经内置 LucidServer、LucidSettings 和 LucidTextService。它会在 TSF 注册时请求管理员权限，但后台服务仍以当前用户权限运行。安装时不需要 PowerShell。
这是未签名测试包，Windows 可能提示未知发布者。
Lucid 只注册当前用户的英语文本服务，不安装键盘钩子。
