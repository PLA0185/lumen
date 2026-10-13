param(
    [string]$KeystorePath = (Join-Path $env:USERPROFILE '.android\debug.keystore')
)

$resolvedPath = (Resolve-Path -LiteralPath $KeystorePath -ErrorAction Stop).Path
$bytes = [System.IO.File]::ReadAllBytes($resolvedPath)
if ($bytes.Length -lt 1024) {
    throw 'Keystore 文件过短，拒绝复制。'
}

$encoded = [Convert]::ToBase64String($bytes)
Set-Clipboard -Value $encoded
Write-Host "Keystore Base64 已复制到剪贴板（文件：$resolvedPath）；没有输出密钥内容。"
