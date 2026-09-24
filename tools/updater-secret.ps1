<#
.SYNOPSIS
  Lumen 更新签名密钥的本地管理：密码用 Windows DPAPI 加密保存，并提供一条命令完成发布构建。

.DESCRIPTION
  整改任务书（第二轮 §9）指出：把**加密私钥**与**明文密码**并排放在同一个目录里，
  会明显降低加密的意义——拿到文件的人同时也拿到了口令。

  本脚本的处置方式：
  - 密码用 Windows DPAPI（`ConvertFrom-SecureString`，未指定 -Key 即为当前用户范围）
    加密后存成 `lumen-updater-password.dpapi`；
  - DPAPI 密文与当前 Windows 账户绑定：换用户、换机器、拷走文件都无法解密；
  - 不再保留任何明文密码文件。

  仍然要如实说明的边界：**同一台机器、同一个登录账户下的任何进程**都能解密这份
  密文。它防的是"文件被拷走 / 被别的账户读到 / 误提交进仓库"，
  而不是本机同账户的恶意进程——那种情况下应当改用硬件密钥或 CI 的 Secret。

  CI 上不使用本脚本：GitHub Actions 用 `secrets.TAURI_SIGNING_PRIVATE_KEY`
  与 `secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，见 `.github/workflows/release.yml`。

.EXAMPLE
  pwsh -File tools/updater-secret.ps1 -Status
      查看私钥与密码的配置状态（不会打印密码）。

.EXAMPLE
  pwsh -File tools/updater-secret.ps1 -Set
      交互式录入密码（不回显），DPAPI 加密后落盘。

.EXAMPLE
  pwsh -File tools/updater-secret.ps1 -Build
      读取私钥与密码，设置签名环境变量后执行 `pnpm tauri build`。

.EXAMPLE
  pwsh -File tools/updater-secret.ps1 -BuildArgs @('-v')
      额外把参数透传给 tauri build。
#>

[CmdletBinding(DefaultParameterSetName = 'Status')]
param(
  # 交互式录入并保存密码（DPAPI 加密）
  [Parameter(ParameterSetName = 'Set')]
  [switch]$Set,

  # 查看配置状态
  [Parameter(ParameterSetName = 'Status')]
  [switch]$Status,

  # 用已保存的凭据执行一次发布构建
  [Parameter(ParameterSetName = 'Build')]
  [switch]$Build,

  # 删除本脚本管理的密码文件（不动私钥）
  [Parameter(ParameterSetName = 'Clear')]
  [switch]$Clear,

  # 透传给 tauri build 的额外参数
  [Parameter(ParameterSetName = 'Build')]
  [string[]]$BuildArgs = @()
)

$ErrorActionPreference = 'Stop'

# ---- 路径集中在这里定义，避免各处硬编码 ----
$TauriDir = Join-Path $env:USERPROFILE '.tauri'
$KeyPath = Join-Path $TauriDir 'lumen-updater.key'
$SecretPath = Join-Path $TauriDir 'lumen-updater-password.dpapi'
# 历史遗留的明文密码文件（若存在，-Status 会提示清理）
$LegacyPlainPath = Join-Path $TauriDir 'lumen-updater-password.txt'

function Write-Utf8NoBom {
  param([string]$Path, [string]$Text)
  $enc = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Text, $enc)
}

function Get-PlainPassword {
  if (-not (Test-Path -LiteralPath $SecretPath)) { return $null }
  $cipher = (Get-Content -LiteralPath $SecretPath -Raw).Trim()
  if ([string]::IsNullOrWhiteSpace($cipher)) { return $null }
  $sec = ConvertTo-SecureString $cipher
  # SecureString → 明文只在内存里短暂出现，且不会被写进任何文件或日志
  $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
  try {
    return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
  } finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
  }
}

function Show-Status {
  $keyOk = Test-Path -LiteralPath $KeyPath
  $secretOk = Test-Path -LiteralPath $SecretPath
  $legacyOk = Test-Path -LiteralPath $LegacyPlainPath

  Write-Host "私钥：$KeyPath"
  Write-Host ("  " + $(if ($keyOk) { '存在' } else { '**缺失**（没有它就无法发布更新）' }))
  Write-Host "密码（DPAPI）：$SecretPath"
  Write-Host ("  " + $(if ($secretOk) { '存在' } else { '**缺失**（请运行 -Set 录入）' }))

  if ($secretOk) {
    try {
      $null = Get-PlainPassword
      Write-Host '  可解密：是（当前 Windows 账户可以解开）'
    } catch {
      Write-Host "  可解密：**否** —— $($_.Exception.Message)"
      Write-Host '  常见原因：这份密文是在别的用户或别的机器上生成的。'
    }
  }

  if ($legacyOk) {
    Write-Host ''
    Write-Host "警告：仍存在明文密码文件 $LegacyPlainPath" -ForegroundColor Yellow
    Write-Host '      请运行  pwsh -File tools/updater-secret.ps1 -Set  迁移到 DPAPI 后删除它。'
  }

  if (-not $keyOk -and -not $secretOk) { exit 1 }
}

function Set-Secret {
  if (Test-Path -LiteralPath $LegacyPlainPath) {
    Write-Host "检测到历史明文密码文件：$LegacyPlainPath"
    $migrate = Read-Host '是否用它迁移到 DPAPI 加密存储？(y/N)'
    if ($migrate -eq 'y' -or $migrate -eq 'Y') {
      $plain = (Get-Content -LiteralPath $LegacyPlainPath -Raw).Trim()
      $sec = ConvertTo-SecureString $plain -AsPlainText -Force
      Write-Utf8NoBom -Path $SecretPath -Text (ConvertFrom-SecureString $sec)
      Write-Host '已写入 DPAPI 密文。'
      $del = Read-Host "现在删除明文文件？删除后无法从它恢复（建议先确认密码已记牢）(y/N)"
      if ($del -eq 'y' -or $del -eq 'Y') {
        Remove-Item -LiteralPath $LegacyPlainPath -Force
        Write-Host '明文密码文件已删除。'
      } else {
        Write-Host '保留了明文文件。注意：在它被删除前，加密的意义仍然有限。'
      }
      return
    }
  }

  $s1 = Read-Host '请输入更新签名私钥的密码（不会回显）' -AsSecureString
  $s2 = Read-Host '请再输入一次以确认' -AsSecureString
  $p1 = [Runtime.InteropServices.Marshal]::PtrToStringBSTR(
    [Runtime.InteropServices.Marshal]::SecureStringToBSTR($s1))
  $p2 = [Runtime.InteropServices.Marshal]::PtrToStringBSTR(
    [Runtime.InteropServices.Marshal]::SecureStringToBSTR($s2))
  if ($p1 -ne $p2) { throw '两次输入不一致，未做任何修改。' }
  if ([string]::IsNullOrEmpty($p1)) { throw '密码为空——空密码在 Windows 上无法通过环境变量传递。' }

  New-Item -ItemType Directory -Force -Path $TauriDir | Out-Null
  Write-Utf8NoBom -Path $SecretPath -Text (ConvertFrom-SecureString $s1)
  Write-Host "已把密码用 DPAPI 加密写入：$SecretPath"
}

function Start-Build {
  if (-not (Test-Path -LiteralPath $KeyPath)) {
    throw "找不到签名私钥：$KeyPath（没有它无法生成更新签名）"
  }
  $pw = Get-PlainPassword
  if ($null -eq $pw) {
    throw "找不到可用的密码密文：$SecretPath（请先运行 -Set）"
  }

  # 私钥内容通过环境变量传给 tauri；密码同样只在进程环境里出现
  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -LiteralPath $KeyPath -Raw).Trim()
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $pw

  Write-Host '已载入签名私钥与密码，开始构建…'
  try {
    & pnpm tauri build @BuildArgs
    if ($LASTEXITCODE -ne 0) { throw "构建失败，退出码 $LASTEXITCODE" }
  } finally {
    # 构建结束就清掉环境变量，避免在同一次会话里被后续命令读到
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
  }
}

switch ($PSCmdlet.ParameterSetName) {
  'Set' { Set-Secret }
  'Build' { Start-Build }
  'Clear' {
    if (Test-Path -LiteralPath $SecretPath) {
      Remove-Item -LiteralPath $SecretPath -Force
      Write-Host "已删除 $SecretPath"
    } else {
      Write-Host '没有需要删除的密码文件。'
    }
  }
  default { Show-Status }
}
