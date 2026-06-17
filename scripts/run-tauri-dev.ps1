# scripts/run-tauri-dev.ps1 — launch `npm run tauri dev` detached, then verify.

$ErrorActionPreference = 'Stop'
$repo = $PSScriptRoot | Split-Path -Parent
$logVite = "$env:TEMP\pm-tauri-vite.log"
$logTauri = "$env:TEMP\pm-tauri-rust.log"
Remove-Item $logVite, $logTauri -ErrorAction SilentlyContinue

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
  $env:PATH = "$cargoBin;$env:PATH"
}

Write-Host "[1/4] launching tauri dev detached…"
$t = Start-Process -FilePath "cmd.exe" `
  -ArgumentList "/c", "set PATH=$cargoBin;%PATH%&& npm run tauri dev > `"$logTauri`" 2>&1" `
  -PassThru `
  -WorkingDirectory $repo `
  -WindowStyle Hidden

Write-Host "  parent pid = $($t.Id)"
Write-Host "[2/4] waiting for vite to start on :1420 (max 90s)…"

$ok = $false
for ($i = 0; $i -lt 90; $i++) {
  Start-Sleep -Seconds 1
  try {
    $res = Invoke-WebRequest -Uri "http://127.0.0.1:1420/" -UseBasicParsing -TimeoutSec 2
    if ($res.StatusCode -eq 200) {
      Write-Host "  vite responding after ${i}s — status $($res.StatusCode)"
      $ok = $true
      break
    }
  } catch {
    # not yet
  }
}
if (-not $ok) {
  Write-Host "  vite did NOT come up. Tauri log:" -ForegroundColor Yellow
  if (Test-Path $logTauri) { Get-Content $logTauri -Tail 40 }
  exit 1
}

Write-Host "[3/4] waiting for the tauri binary to launch (max 60s)…"
$bin = $null
for ($i = 0; $i -lt 60; $i++) {
  Start-Sleep -Seconds 1
  $bin = Get-Process -Name "puppet_master_app" -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($bin) { Write-Host "  binary running — pid $($bin.Id), ${i}s after vite"; break }
}
if (-not $bin) {
  Write-Host "  binary did NOT launch. Tauri log tail:" -ForegroundColor Yellow
  if (Test-Path $logTauri) { Get-Content $logTauri -Tail 60 }
  exit 1
}

Write-Host "[4/4] tail of tauri log:"
if (Test-Path $logTauri) { Get-Content $logTauri -Tail 25 }
Write-Host ""
Write-Host "TAURI DEV IS UP" -ForegroundColor Green
Write-Host "  vite:      http://127.0.0.1:1420"
Write-Host "  binary:    pid $($bin.Id)"
Write-Host "  parent:    pid $($t.Id)"
Write-Host "  log files: $logVite, $logTauri"
Write-Host ""
Write-Host "to stop:  Stop-Process -Id $($t.Id)"
