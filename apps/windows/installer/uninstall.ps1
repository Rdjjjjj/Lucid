# 移除 Lucid 的当前用户文本服务、Server 启动项和程序文件。
$ErrorActionPreference = "Stop"
$dest = Join-Path $env:LOCALAPPDATA "Lucid"
$settings = Join-Path $dest "LucidSettings.exe"
if (Test-Path -LiteralPath $settings -PathType Leaf) {
    $process = Start-Process -FilePath $settings -ArgumentList "--unregister" -Wait -PassThru -NoNewWindow
    if ($process.ExitCode -ne 0) { throw "注销 TSF 失败，退出码 $($process.ExitCode)" }
}
Get-Process -Name "LucidServer" -ErrorAction SilentlyContinue | Stop-Process -Force
$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Remove-ItemProperty -Path $runKey -Name "LucidServer" -ErrorAction SilentlyContinue
if (Test-Path -LiteralPath $dest) { Remove-Item -Recurse -Force -LiteralPath $dest }
Write-Host "Lucid 已卸载。"
