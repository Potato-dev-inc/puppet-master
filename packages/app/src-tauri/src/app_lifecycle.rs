//! App install lifecycle: version metadata, external links, and uninstall.

use serde::Serialize;
use std::process::Command;

#[cfg(target_os = "macos")]
use std::path::PathBuf;

const PRODUCT_NAME: &str = "Puppet Master";
const BUNDLE_ID: &str = "com.puppetmaster.app";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInstallInfo {
    pub version: String,
    pub is_packaged: bool,
    pub platform: String,
    pub uninstall_available: bool,
    pub uninstall_instructions: String,
    pub data_dir: String,
}

#[tauri::command]
pub fn get_app_install_info() -> AppInstallInfo {
    let platform = std::env::consts::OS.to_string();
    let is_packaged = is_packaged_install();
    AppInstallInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        is_packaged,
        platform: platform.clone(),
        uninstall_available: uninstall_supported(&platform, is_packaged),
        uninstall_instructions: uninstall_instructions(&platform),
        data_dir: crate::app_paths::path_for_host_config(&crate::app_paths::app_data_dir()),
    }
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("only http(s) URLs are allowed".into());
    }
    open_url(trimmed)
}

#[tauri::command]
pub async fn launch_uninstall(app: tauri::AppHandle) -> Result<(), String> {
    if !is_packaged_install() {
        return Err(
            "Uninstall is only available from an installed release build (not dev or cargo run)."
                .into(),
        );
    }

    #[cfg(target_os = "windows")]
    {
        let uninstall = find_windows_uninstall_string()?;
        Command::new("cmd.exe")
            .args(["/C", &uninstall])
            .spawn()
            .map_err(|err| format!("could not start uninstaller: {err}"))?;
        app.exit(0);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let app_path = default_macos_app_path();
        if !app_path.exists() {
            return Err(format!(
                "could not find {} in /Applications",
                PRODUCT_NAME
            ));
        }
        let script = format!(
            "tell application \"Finder\" to delete POSIX file \"{}\"",
            app_path.display()
        );
        let status = Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map_err(|err| format!("could not run uninstall: {err}"))?;
        if !status.success() {
            return Err("macOS uninstall was cancelled or failed".into());
        }
        app.exit(0);
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = app;
        Err("In-app uninstall is not supported on this platform. Remove the package with your system package manager.".into())
    }
}

fn is_packaged_install() -> bool {
    std::env::current_exe()
        .map(|path| {
            let normalized = path.to_string_lossy().replace('\\', "/").to_lowercase();
            !normalized.contains("/target/debug/")
                && !normalized.contains("/target/release/")
                && !normalized.contains("/target\\debug\\")
                && !normalized.contains("/target\\release\\")
        })
        .unwrap_or(false)
}

fn uninstall_supported(platform: &str, is_packaged: bool) -> bool {
    is_packaged && matches!(platform, "windows" | "macos")
}

fn uninstall_instructions(platform: &str) -> String {
    match platform {
        "windows" => format!(
            "Installed build: use Uninstall below, or Windows Settings → Apps → {PRODUCT_NAME}.\n\
             Dev build: close the app and delete the repo folder.\n\
             App data (settings, pairing) may remain in %APPDATA%\\{BUNDLE_ID} until you delete it."
        ),
        "macos" => format!(
            "Installed build: use Uninstall below, or drag {PRODUCT_NAME}.app from Applications to Trash.\n\
             Dev build: close the app and delete the repo folder.\n\
             App data may remain in ~/Library/Application Support/{BUNDLE_ID}."
        ),
        _ => format!(
            "Remove the installed package with your system package manager.\n\
             App data may remain under the {BUNDLE_ID} config directory."
        ),
    }
    .to_string()
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new("cmd.exe")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|err| format!("could not open URL: {err}"))?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|err| format!("could not open URL: {err}"))?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|err| format!("could not open URL: {err}"))?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = url;
        Err("opening URLs is not supported on this platform".into())
    }
}

#[cfg(target_os = "windows")]
fn find_windows_uninstall_string() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let script = format!(
        r#"$roots = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
$entry = foreach ($root in $roots) {{
  Get-ItemProperty $root -ErrorAction SilentlyContinue |
    Where-Object {{
      $_.DisplayName -eq '{PRODUCT_NAME}' -or $_.DisplayName -like '{PRODUCT_NAME} *' -or $_.BundleId -eq '{BUNDLE_ID}'
    }} |
    Select-Object -First 1
}}
if ($entry -and $entry.UninstallString) {{ $entry.UninstallString }} else {{ exit 1 }}"#
    );
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|err| format!("could not query uninstall registry: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "could not find {PRODUCT_NAME} in Windows Apps & features. \
             Open Settings → Apps and uninstall manually."
        ));
    }
    let uninstall = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if uninstall.is_empty() {
        return Err("Windows uninstall entry was empty".into());
    }
    Ok(uninstall)
}

#[cfg(target_os = "macos")]
fn default_macos_app_path() -> PathBuf {
    PathBuf::from(format!("/Applications/{PRODUCT_NAME}.app"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_install_detects_target_folder() {
        assert!(!is_packaged_install() || std::env::current_exe().is_ok());
    }

    #[test]
    fn uninstall_instructions_include_product_name() {
        let text = uninstall_instructions("windows");
        assert!(text.contains(PRODUCT_NAME));
    }
}
