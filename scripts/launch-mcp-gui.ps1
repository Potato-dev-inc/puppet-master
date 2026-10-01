$ErrorActionPreference = 'Stop'
$repoPath = Split-Path $PSScriptRoot -Parent
$pythonPath = Join-Path $repoPath '.venv-qt-mcp/Scripts/pythonw.exe'
$launcherPath = Join-Path $PSScriptRoot 'puppet-master-mcp-gui.py'
if (-not (Test-Path -LiteralPath $pythonPath)) {
    Write-Error 'Set up the GUI first: python -m venv .venv-qt-mcp; .venv-qt-mcp/Scripts/python.exe -m pip install -r scripts/qt_mcp/requirements.txt'
}
Start-Process -FilePath $pythonPath -ArgumentList @(('"' + $launcherPath + '"')) -WorkingDirectory $repoPath -WindowStyle Normal
