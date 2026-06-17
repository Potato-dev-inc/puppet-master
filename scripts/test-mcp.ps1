$pf = "$env:TEMP\pm-mcp-test.port"
Remove-Item $pf -ErrorAction SilentlyContinue
$errLog = "$env:TEMP\pm-mcp-bridge.err"
Remove-Item $errLog -ErrorAction SilentlyContinue
$mcpErrLog = "$env:TEMP\pm-mcp-mcp.err"
Remove-Item $mcpErrLog -ErrorAction SilentlyContinue

# Start bridge
$env:PUPPET_MASTER_BRIDGE_PORT_FILE = $pf
$bridge = Start-Process -FilePath "node" `
  -ArgumentList "--enable-source-maps", "packages\bridge\dist\server.js" `
  -PassThru -NoNewWindow `
  -RedirectStandardError $errLog `
  -WorkingDirectory "C:\Users\Ren-pc\Desktop\work\tmux-puppet-master"

Start-Sleep -Milliseconds 800
$port = (Get-Content $pf).Trim().Split(':')[-1]
Write-Host "bridge on port $port"

# Start MCP server with same env so it finds the bridge
$mcp = Start-Process -FilePath "node" `
  -ArgumentList "--enable-source-maps", "packages\mcp-server\dist\index.js" `
  -PassThru -NoNewWindow `
  -RedirectStandardError $mcpErrLog `
  -RedirectStandardInput "C:\Users\Ren-pc\Desktop\work\tmux-puppet-master\scripts\mcp-input.txt" `
  -WorkingDirectory "C:\Users\Ren-pc\Desktop\work\tmux-puppet-master"

Start-Sleep -Milliseconds 800
Write-Host "--- mcp stderr ---"
Get-Content $mcpErrLog

Stop-Process -Id $mcp.Id, $bridge.Id -Force -ErrorAction SilentlyContinue
Remove-Item $pf -ErrorAction SilentlyContinue
