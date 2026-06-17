$pf = "$env:TEMP\pm-bridge-test.port"
Remove-Item $pf -ErrorAction SilentlyContinue
$env:PUPPET_MASTER_BRIDGE_PORT_FILE = $pf
$errLog = "$env:TEMP\pm-bridge.err"
Remove-Item $errLog -ErrorAction SilentlyContinue

$bridge = Start-Process -FilePath "node" `
  -ArgumentList "--enable-source-maps", "packages\bridge\dist\server.js" `
  -PassThru -NoNewWindow `
  -RedirectStandardError $errLog `
  -WorkingDirectory "C:\Users\Ren-pc\Desktop\work\tmux-puppet-master"

Start-Sleep -Milliseconds 800

if (-not (Test-Path $pf)) {
  Write-Host "PORT FILE NOT WRITTEN"
  Get-Content $errLog
  Stop-Process -Id $bridge.Id -Force -ErrorAction SilentlyContinue
  exit 1
}

$port = (Get-Content $pf).Trim().Split(':')[-1]
Write-Host "bridge on port $port"
Write-Host ""

Write-Host "--- GET /health ---"
(Invoke-WebRequest -Uri "http://127.0.0.1:$port/health" -UseBasicParsing -TimeoutSec 3).Content

Write-Host ""
Write-Host "--- POST /panes (powershell) ---"
$r1 = Invoke-WebRequest -Uri "http://127.0.0.1:$port/panes" -Method POST -ContentType "application/json" -Body '{"agent_type":"powershell","cols":120,"rows":30}' -UseBasicParsing -TimeoutSec 3
$r1.Content
$paneId = ($r1.Content | ConvertFrom-Json).pane_id

Write-Host ""
Write-Host "--- GET /panes ---"
(Invoke-WebRequest -Uri "http://127.0.0.1:$port/panes" -UseBasicParsing -TimeoutSec 3).Content

Write-Host ""
Write-Host "--- POST /panes (claude) ---"
(Invoke-WebRequest -Uri "http://127.0.0.1:$port/panes" -Method POST -ContentType "application/json" -Body '{"agent_type":"claude","cols":80,"rows":24}' -UseBasicParsing -TimeoutSec 3).Content

Write-Host ""
Write-Host "--- DELETE /panes/$paneId ---"
(Invoke-WebRequest -Uri "http://127.0.0.1:$port/panes/$paneId" -Method DELETE -UseBasicParsing -TimeoutSec 3).Content

Write-Host ""
Write-Host "--- GET /panes after kill ---"
(Invoke-WebRequest -Uri "http://127.0.0.1:$port/panes" -UseBasicParsing -TimeoutSec 3).Content

Write-Host ""
Write-Host "--- POST /project-path ---"
(Invoke-WebRequest -Uri "http://127.0.0.1:$port/project-path" -Method POST -ContentType "application/json" -Body '{"path":"C:\\Users\\Ren-pc\\Desktop\\work\\tmux-puppet-master"}' -UseBasicParsing -TimeoutSec 3).Content

Stop-Process -Id $bridge.Id -Force -ErrorAction SilentlyContinue
Remove-Item $pf -ErrorAction SilentlyContinue
Write-Host ""
Write-Host "ALL OK"
