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

  历史明文文件 `lumen-updater-password.txt` 的迁移语义（默认安全）：
  - `-Set` 在写入密文后会立刻做一次**解密往返校验**（读回密文并解密，结果必须与原文一致）；
  - 校验通过后**立即删除**该明文文件，不再询问"是否保留"——迁移即删除；
  - 校验失败时**中止迁移且不删除明文**（宁可留明文，也不能留下"看起来迁移成功、
    其实解不开"的密文），打印原因并以非零退出码结束；
  - 删除明文失败时，同样视为**迁移未完成**，打印明确告警并以非零退出码结束；
  - `-Status` 逐项检查四件事，**任何一项不达标都 exit 1**（第三轮收口任务书 §22）：
    ① 私钥存在；② 密码密文存在；③ 密文能用当前账户解开；④ 没有 legacy 明文文件残留。
    因此 `-Status` 退出码 0 的含义是：四项全部健康——私钥与密文就绪、密文可解密、
    且没有任何明文密码文件残留。此前"只有私钥与密文**同时**缺失才非零、解密失败只打印
    一行提示"的写法会让"其实已经坏了"的状态（私钥缺失但密文存在、换过账户导致密文
    解不开）返回 0，现按四条件独立计分，任一不满足即 exit 1。

  仍然要如实说明的边界：**同一台机器、同一个登录账户下的任何进程**都能解密这份
  密文。它防的是"文件被拷走 / 被别的账户读到 / 误提交进仓库"，
  而不是本机同账户的恶意进程——那种情况下应当改用硬件密钥或 CI 的 Secret。

  CI 上不使用本脚本：GitHub Actions 用 `secrets.TAURI_SIGNING_PRIVATE_KEY`
  与 `secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，见 `.github/workflows/release.yml`。

.EXAMPLE
  pwsh -File tools/updater-secret.ps1 -Status
      查看私钥与密码的配置状态（不会打印密码）。
      退出码：私钥存在 + 密文存在 + 能解密 + 无 legacy 明文，四项全健康才 0，否则 1。

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
# 历史遗留的明文密码文件（若存在，-Status 会标记为不安全并以非零退出码结束）
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
  # 四项健康检查各自独立计分，任何一项不达标都不能退出 0（任务书 §22）
  $decryptFailed = $false
  $exitCode = 0

  Write-Host "私钥：$KeyPath"
  Write-Host ("  " + $(if ($keyOk) { '存在' } else { '**缺失**（没有它就无法发布更新）' }))
  Write-Host "密码（DPAPI）：$SecretPath"
  Write-Host ("  " + $(if ($secretOk) { '存在' } else { '**缺失**（请运行 -Set 录入）' }))

  if ($secretOk) {
    try {
      $plain = Get-PlainPassword
      if ($null -eq $plain) {
        # 文件在、内容却是空的或不可识别的密文：效果等同于解密失败，不能算健康
        $decryptFailed = $true
        Write-Host '  可解密：**否** —— 密文内容为空或不可识别。'
        Write-Host '  常见原因：文件被截断或写坏；请运行 -Set 重新录入密码。'
      } else {
        Write-Host '  可解密：是（当前 Windows 账户可以解开）'
      }
    } catch {
      $decryptFailed = $true
      Write-Host "  可解密：**否** —— $($_.Exception.Message)"
      Write-Host '  常见原因：这份密文是在别的用户或别的机器上生成的。'
    }
  }

  if ($legacyOk) {
    Write-Host ''
    Write-Host "不安全：仍存在明文密码文件 $LegacyPlainPath" -ForegroundColor Yellow
    Write-Host '      只要它还在，"密码已加密保存"就没有实际意义——拿到该文件的人同时拿到了口令。'
    Write-Host '      请运行  pwsh -File tools/updater-secret.ps1 -Set  完成迁移（校验通过后会立即删除它）。'
  }

  if (-not $keyOk) { $exitCode = 1 }
  if (-not $secretOk) { $exitCode = 1 }
  if ($decryptFailed) { $exitCode = 1 }
  if ($legacyOk) { $exitCode = 1 }

  if ($exitCode -ne 0) {
    Write-Host ''
    Write-Host '状态不健康：上面凡标记为缺失 / 不可解密 / 不安全的项，都会让 -Status 以退出码 1 结束。' -ForegroundColor Yellow
  }

  exit $exitCode
}

function Set-Secret {
  if (Test-Path -LiteralPath $LegacyPlainPath) {
    Write-Host "检测到历史明文密码文件：$LegacyPlainPath"
    $migrate = Read-Host '是否用它迁移到 DPAPI 加密存储？迁移成功后会立即删除明文文件（y/N）'
    if ($migrate -eq 'y' -or $migrate -eq 'Y') {
      $plain = (Get-Content -LiteralPath $LegacyPlainPath -Raw).Trim()
      if ([string]::IsNullOrEmpty($plain)) {
        throw "明文密码文件是空的：$LegacyPlainPath —— 请先确认其内容再重试。"
      }

      # 覆盖前先留住旧密文，万一新密文校验不过可以回滚，不留下解不开的密文
      $backupCipher = $null
      if (Test-Path -LiteralPath $SecretPath) {
        $backupCipher = Get-Content -LiteralPath $SecretPath -Raw
      }

      New-Item -ItemType Directory -Force -Path $TauriDir | Out-Null
      $sec = ConvertTo-SecureString $plain -AsPlainText -Force
      Write-Utf8NoBom -Path $SecretPath -Text (ConvertFrom-SecureString $sec)

      # 往返校验：立刻读回密文并解密，必须与明文文件内容完全一致
      $roundTrip = $null
      $verifyError = $null
      try {
        $roundTrip = Get-PlainPassword
      } catch {
        $verifyError = $_.Exception.Message
      }

      if ($verifyError -or $roundTrip -ne $plain) {
        if ($null -ne $backupCipher) {
          Write-Utf8NoBom -Path $SecretPath -Text $backupCipher
          Write-Host '回滚：已恢复覆盖前的 DPAPI 密文。' -ForegroundColor Yellow
        } else {
          Remove-Item -LiteralPath $SecretPath -Force -ErrorAction SilentlyContinue
          Write-Host '回滚：已删除校验失败的 DPAPI 密文。' -ForegroundColor Yellow
        }
        # 宁可留明文，也不能留下"看起来迁移成功、其实解不开"的密文
        Write-Host "安全迁移未完成：DPAPI 密文往返校验失败，已中止，明文密码文件未被删除（$LegacyPlainPath）。" -ForegroundColor Red
        if ($verifyError) { Write-Host "  读回密文时报错：$verifyError" }
        else { Write-Host '  读回的密码与明文文件内容不一致。' }
        Write-Host '  请检查 %USERPROFILE%\.tauri 目录的权限与磁盘状态后重新运行 -Set。'
        exit 1
      }
      Write-Host "往返校验通过，已写入 DPAPI 密文：$SecretPath"

      # 迁移即删除：不再询问是否保留明文；删除失败一律视为迁移未完成
      try {
        Remove-Item -LiteralPath $LegacyPlainPath -Force -ErrorAction Stop
      } catch {
        Write-Host "安全迁移未完成：明文密码文件仍然存在（$LegacyPlainPath）。请手动删除它后重新运行 -Status 确认。" -ForegroundColor Red
        Write-Host "  删除失败原因：$($_.Exception.Message)"
        exit 1
      }
      if (Test-Path -LiteralPath $LegacyPlainPath) {
        Write-Host "安全迁移未完成：明文密码文件仍然存在（$LegacyPlainPath）。请手动删除它后重新运行 -Status 确认。" -ForegroundColor Red
        exit 1
      }

      Write-Host '明文密码文件已删除，迁移完成。'
      return
    }

    Write-Host "已跳过迁移：明文文件仍在（$LegacyPlainPath），它存在期间 -Status 会以非零退出码提示不安全。" -ForegroundColor Yellow
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
