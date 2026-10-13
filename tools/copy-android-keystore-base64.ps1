param(
    [string]$KeystorePath = (Join-Path $env:USERPROFILE '.android\debug.keystore')
)

$resolvedPath = (Resolve-Path -LiteralPath $KeystorePath -ErrorAction Stop).Path
$bytes = [System.IO.File]::ReadAllBytes($resolvedPath)
if ($bytes.Length -lt 1024) {
    throw 'Keystore file is too short; refusing to copy.'
}

$encoded = [Convert]::ToBase64String($bytes)
Set-Clipboard -Value $encoded
Write-Host "Keystore Base64 copied to clipboard (file: $resolvedPath); key contents were not printed."
