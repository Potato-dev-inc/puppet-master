# Python Qt6 MCP console

Use this desktop console to interact with Puppet Master as an MCP client. It
launches a local stdio MCP server, performs the MCP handshake, discovers its tools,
and lets you submit actual `tools/call` requests. The Puppet Master desktop app
must be running for bridge-backed tools to work.

## Launch on Windows

From the repository root:

```powershell
python -m venv .venv-qt-mcp
.venv-qt-mcp/Scripts/python.exe -m pip install -r scripts/qt_mcp/requirements.txt
.venv-qt-mcp/Scripts/python.exe scripts/puppet-master-mcp-gui.py
```

After setup, launch without a console window:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/launch-mcp-gui.ps1
```

On other platforms, use your virtual environment's Python to run the same Python
launcher. [PySide6](https://doc.qt.io/qtforpython-6/gettingstarted.html) includes the
Qt binaries; no separate Qt installation is needed.

## Use the console

1. Click **Connect**. The console chooses the freshly built Rust debug server,
   followed by the packaged Rust server and the Node launcher.
2. New connections start in **Agent** mode. Click **Choose project**, then
   **Run agent** to submit a task through a typed form.
3. Use **List agents**, **Wait agents**, **Cancel agent**, and **Transcript** to
   inspect or manage the returned handle. A bounded wait timing out leaves the
   worker running; the worker's wall-clock deadline is separate.
4. Click **Shell** for terminal tools or **Both** for the full catalog. The tool
   buttons refresh automatically when the server changes modes. **Take over**
   grants explicit terminal control; preexisting user panes require an explicit grant.
5. Use **Answer prompt** for the current prompt ID and an explicit choice. The
   console never approves prompts automatically. **Release** returns a taken-over
   pane to its agent owner.

The main window contains action buttons and read-only results. Every discovered
tool has a button; tools needing arguments open ordinary forms. There is no JSON
editor, schema browser, or command configuration. If the operation tools are
missing in **Both** mode, rebuild the current Rust MCP executable and reconnect.

**Cancel request** cancels the most recent pending MCP request (for example, a wait).
**Cancel operation** calls `cancel_operation` to stop the delegated work. Closing
or disconnecting the console shuts down only the MCP child it launched; it does
not cancel delegated operations or stop Puppet Master Desktop.

After building these changes, restart Puppet Master Desktop when existing workers
can safely stop, then reconnect this console. A running desktop process keeps its
previous bridge implementation until restarted.

Mutating tools run only when you explicitly submit them. Retrying identical
delegation arguments reuses the request key to avoid duplicate work. Results may
contain project data; nothing is uploaded by the console.
Click **New task** before intentionally repeating identical work. It resets retry
and tracking context without stopping existing worker operations.

## Tests

```powershell
.venv-qt-mcp/Scripts/python.exe -m unittest discover -s scripts/qt_mcp -p 'test_*.py'
```

The tests use mock stdio MCP processes and offscreen Qt widgets. They do not
launch paid workers or automatically delegate real work.
