param(
  [Parameter(Mandatory=$true)][string]$Executable,
  [string]$AlternateExecutable
)
$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $Executable).Path
$initial = @(Get-Process lumen -ErrorAction SilentlyContinue)
if ($initial.Count -ne 1 -or $initial[0].Path -ne $resolved) {
  throw 'Start the specified executable first. Exactly one Lumen process is required.'
}
$firstPid = $initial[0].Id
$launchPaths = @($resolved, $resolved, $resolved)
if ($AlternateExecutable) { $launchPaths += (Resolve-Path -LiteralPath $AlternateExecutable).Path }
foreach ($launchPath in $launchPaths) {
  $second = Start-Process -FilePath $launchPath -WindowStyle Hidden -PassThru
  if (!$second.WaitForExit(10000)) { throw "Duplicate launch did not exit: $($second.Id)" }
  $remaining = @(Get-Process lumen -ErrorAction SilentlyContinue)
  if ($remaining.Count -ne 1 -or $remaining[0].Id -ne $firstPid) {
    throw 'Duplicate launch did not preserve the original single process'
  }
}
Write-Output 'PASS: repeated launches preserve the original single Lumen process'
