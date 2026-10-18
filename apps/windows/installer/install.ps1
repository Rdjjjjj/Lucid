# Lucid Windows 安装脚本（当前用户，无需管理员权限）。
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA "Lucid"
$settings = Join-Path $here "LucidSettings.exe"
$server = Join-Path $here "LucidServer.exe"
$dll = Join-Path $here "LucidTextService.dll"
foreach ($file in @($settings, $server, $dll)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        throw "找不到 $([IO.Path]::GetFileName($file))，请确认安装包已完整解压。"
    }
}

New-Item -ItemType Directory -Force -Path $dest | Out-Null
foreach ($file in @($settings, $server, $dll)) {
    Copy-Item -Force -LiteralPath $file -Destination (Join-Path $dest ([IO.Path]::GetFileName($file)))
}
$manifest = Join-Path $here "LucidSettings.exe.manifest"
if (Test-Path -LiteralPath $manifest -PathType Leaf) {
    Copy-Item -Force -LiteralPath $manifest -Destination (Join-Path $dest "LucidSettings.exe.manifest")
}

$destSettings = Join-Path $dest "LucidSettings.exe"
$destServer = Join-Path $dest "LucidServer.exe"
$destDll = Join-Path $dest "LucidTextService.dll"
Get-Process -Name "LucidServer" -ErrorAction SilentlyContinue | Stop-Process -Force
$process = Start-Process -FilePath $destSettings `
    -ArgumentList @("--register", ('"{0}"' -f $destDll)) `
    -Wait -PassThru -NoNewWindow
if ($process.ExitCode -ne 0) { throw "注册 TSF 失败，退出码 $($process.ExitCode)" }

$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
New-Item -Path $runKey -Force | Out-Null
Set-ItemProperty -Path $runKey -Name "LucidServer" -Value ('"{0}"' -f $destServer)
Start-Process -FilePath $destServer | Out-Null

Write-Host "Lucid 已安装到 $dest"
Write-Host "请打开 LucidSettings.exe 配置模型，然后在 设置 -> 时间和语言 -> 输入 里添加 Lucid。"
