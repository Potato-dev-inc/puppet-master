use base64::Engine;

pub(super) fn build(agent: &str, command: &str, marker: &str, cwd: Option<&str>) -> String {
    let start_marker = format!("{marker}_START");
    match agent.to_ascii_lowercase().as_str() {
        "powershell" => powershell(command, marker, &start_marker, cwd),
        "cmd" => format!(
            "echo {start_marker}\r{}\rset \"__pm_ec=%errorlevel%\"\recho {marker}^|%__pm_ec%^|%cd%",
            cmd_with_cwd(command, cwd)
        ),
        "bash" => format!(
            "printf '{start_marker}\\n'\n{}\n__pm_ec=$?\nprintf '\\n{marker}|%s|%s\\n' \"$__pm_ec\" \"$(pwd | base64 | tr -d '\\r\\n')\"",
            bash_with_cwd(command, cwd)
        ),
        _ => String::new(),
    }
}

fn powershell(command: &str, marker: &str, start_marker: &str, cwd: Option<&str>) -> String {
    let command = powershell_with_cwd(command, cwd);
    let utf16le = command
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16le);
    format!(
        "[Console]::WriteLine('{start_marker}'); $global:LASTEXITCODE = 0; $pmSw = [Diagnostics.Stopwatch]::StartNew(); $pmCode = 0; try {{ Invoke-Expression ([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{encoded}'))); $pmOk = $?; $pmNative = $global:LASTEXITCODE; $pmCode = if ($null -ne $pmNative -and $pmNative -ne 0) {{ [int]$pmNative }} elseif ($pmOk) {{ 0 }} else {{ 1 }} }} catch {{ $pmCode = 1; [Console]::Error.WriteLine($_) }}; $pmSw.Stop(); [Console]::WriteLine('{marker}|' + $pmCode + '|' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((Get-Location).Path)))"
    )
}

pub(super) fn powershell_with_cwd(command: &str, cwd: Option<&str>) -> String {
    cwd.map(|cwd| {
        format!(
            "Set-Location -LiteralPath '{}' -ErrorAction Stop;\n{}",
            cwd.replace('\'', "''"),
            command
        )
    })
    .unwrap_or_else(|| command.to_string())
}

fn cmd_with_cwd(command: &str, cwd: Option<&str>) -> String {
    let command = command.replace("\r\n", "\n").replace('\n', "\r");
    cwd.map(|cwd| format!("cd /d \"{cwd}\"\r{command}"))
        .unwrap_or(command)
}

fn bash_with_cwd(command: &str, cwd: Option<&str>) -> String {
    let command = command.replace("\r\n", "\n");
    cwd.map(|cwd| format!("cd -- '{}' &&\n{command}", cwd.replace('\'', "'\\''")))
        .unwrap_or(command)
}

pub(super) fn parse_completion(output: &str, marker: &str, agent: &str) -> Option<(i32, String)> {
    let suffix = output
        .lines()
        .rev()
        .filter_map(|line| line.trim().strip_prefix(marker))
        .find(|suffix| suffix.starts_with('|'))?
        .trim_start_matches('|');
    let mut fields = suffix.split('|');
    let exit_code = fields.next()?.parse::<i32>().ok()?;
    let cwd = fields.next()?;
    if agent.eq_ignore_ascii_case("cmd") {
        return Some((exit_code, cwd.trim().to_string()));
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(cwd.trim())
        .ok()?;
    Some((exit_code, String::from_utf8(decoded).ok()?))
}

pub(super) fn output_for_command(output: &str, marker: &str) -> String {
    let start_marker = format!("{marker}_START");
    // Interactive shells may leave one or more stale keystrokes at the start
    // of the line that prints our marker. Accept only a line ending in the
    // unique marker; an echoed wrapper includes additional text after it.
    let Some(start_line) = output
        .lines()
        .position(|line| line.trim_end().ends_with(&start_marker))
    else {
        return String::new();
    };
    output
        .lines()
        .skip(start_line + 1)
        .take_while(|line| !line.trim().starts_with(marker))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
