use rspm_common::Result;

#[cfg(target_os = "windows")]
pub async fn install_service(output: bool) -> Result<()> {
    use std::path::PathBuf;

    let rspm_dir = dirs::home_dir()
        .map(|p| p.join(".rspm"))
        .unwrap_or_else(|| PathBuf::from(".rspm"));

    let rspmd_path = std::env::current_exe()
        .map(|p| p.parent().map(|p| p.join("rspmd.exe")).unwrap_or_default())
        .unwrap_or_else(|_| PathBuf::from("rspmd.exe"));

    let script = format!(
        r#"@echo off
:: Windows Service Installation Script for RSPM Daemon (rspmd)
:: Uses Windows built-in sc command

setlocal enabledelayedexpansion

set "RSPM_DIR={}"
set "RSPMD_EXE={}"

echo ========================================
echo   RSPM Daemon Service Installation
echo ========================================
echo.
echo RSPM Directory: %RSPM_DIR%
echo RSPMD Executable: %RSPMD_EXE%
echo.

:: Create RSPM directories if not exist
if not exist "%RSPM_DIR%" mkdir "%RSPM_DIR%"
if not exist "%RSPM_DIR%\db" mkdir "%RSPM_DIR%\db"
if not exist "%RSPM_DIR%\logs" mkdir "%RSPM_DIR%\logs"
if not exist "%RSPM_DIR%\pid" mkdir "%RSPM_DIR%\pid"
if not exist "%RSPM_DIR%\sock" mkdir "%RSPM_DIR%\sock"

:: Check if service already exists
sc query rspmd >nul 2>&1
if %ERRORLEVEL% equ 0 (
    echo Service already exists. Stopping and deleting...
    sc stop rspmd >nul 2>&1
    sc delete rspmd >nul 2>&1
)

:: Create service using sc command
:: binPath= - The path to the executable (required, must have equals sign)
:: DisplayName= - The display name
:: Description= - The description
:: start= - Start type (auto, manual, disabled)
sc create rspmd binPath= "%RSPMD_EXE%" DisplayName= "RSPM Process Manager" Description= "Rust Process Manager - Process management daemon" start= auto
if %ERRORLEVEL% neq 0 (
    echo Failed to create service
    exit /b 1
)

:: Set service to restart on failure
sc failure rspmd reset= 86400 actions= restart/60000/restart/60000/restart/60000

echo.
echo Service installed successfully!
echo.
echo To start the service:
echo   net start rspmd
echo   or: sc start rspmd
echo.
echo To stop the service:
echo   net stop rspmd
echo   or: sc stop rspmd
echo.
echo To uninstall:
echo   sc stop rspmd
echo   sc delete rspmd
echo.

endlocal
"#,
        rspm_dir.display(),
        rspmd_path.display()
    );

    if output {
        // Output script to stdout - use PowerShell to ensure correct encoding
        let output = std::process::Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Write-Output @'\n{}\n'@ | Out-File -FilePath install.bat -Encoding ASCII",
                    script
                ),
            ])
            .output()?;

        if output.status.success() {
            println!("Script written to install.bat");
            println!("Run: .\\install.bat");
        } else {
            // Fallback: just print the script
            println!("{}", script);
        }
    } else {
        // Write to a temp file and execute
        let temp_path = std::env::temp_dir().join("rspm-install.bat");

        // Use PowerShell to write with correct encoding
        let ps_script = format!(
            "@'\n{}\n'@ | Out-File -FilePath '{}' -Encoding ASCII",
            script,
            temp_path.display()
        );
        let _ = std::process::Command::new("powershell")
            .args(["-Command", &ps_script])
            .output();

        println!("Installing RSPM daemon as Windows service...");

        let output = std::process::Command::new("cmd")
            .args(["/C", &temp_path.display().to_string()])
            .output()?;

        if output.status.success() {
            println!(
                "{}",
                colored::Colorize::green("Service installed successfully!")
            );
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Installation failed: {}", stderr).as_str())
            );
            return Err(rspm_common::RspmError::InternalError(stderr.to_string()));
        }

        let _ = std::fs::remove_file(temp_path);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub async fn uninstall_service() -> Result<()> {
    println!("Uninstalling RSPM daemon service...");

    // Stop the service first
    let _ = std::process::Command::new("sc")
        .args(["stop", "rspmd"])
        .output();

    // Delete the service
    let output = std::process::Command::new("sc")
        .args(["delete", "rspmd"])
        .output()?;

    if output.status.success() {
        println!(
            "{}",
            colored::Colorize::green("Service uninstalled successfully!")
        );
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Ignore if service doesn't exist
        if stderr.contains("does not exist") || stderr.contains("The specified service") {
            println!(
                "{}",
                colored::Colorize::yellow("Service was not installed.")
            );
        } else {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Uninstallation failed: {}", stderr).as_str())
            );
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub async fn install_service(output: bool) -> Result<()> {
    use std::path::PathBuf;

    let rspm_dir = dirs::home_dir()
        .map(|p| p.join(".rspm"))
        .unwrap_or_else(|| PathBuf::from(".rspm"));

    let rspmd_path = std::env::current_exe()
        .map(|p| p.parent().map(|p| p.join("rspmd")).unwrap_or_default())
        .unwrap_or_else(|_| PathBuf::from("rspmd"));

    let user = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let group = std::process::Command::new("id")
        .args(["-gn"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "root".to_string());

    let service_content = format!(
        r#"[Unit]
Description=RSPM Process Manager
Documentation=https://github.com/vcqr/rspm
After=network.target

[Service]
Type=simple
User={}
Group={}
WorkingDirectory={}
ExecStart={}
Restart=on-failure
RestartSec=5
StandardOutput=append:{}/logs/daemon-out.log
StandardError=append:{}/logs/daemon-err.log
Environment="RSPM_HOME={}"
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths={}

[Install]
WantedBy=default.target
"#,
        user,
        group,
        rspm_dir.display(),
        rspmd_path.display(),
        rspm_dir.display(),
        rspm_dir.display(),
        rspm_dir.display(),
        rspm_dir.display()
    );

    if output {
        println!("{}", service_content);
        println!("\n# To install, run as root:");
        println!("# cp rspmd.service /etc/systemd/system/");
        println!("# systemctl daemon-reload");
        println!("# systemctl enable rspmd");
        println!("# systemctl start rspmd");
    } else {
        let service_dir = dirs::home_dir()
            .map(|p| p.join(".config/systemd/user"))
            .unwrap_or_else(|| PathBuf::from(".config/systemd/user"));

        std::fs::create_dir_all(&service_dir)?;
        let service_path = service_dir.join("rspmd.service");

        std::fs::write(&service_path, &service_content)?;

        println!("Installing RSPM daemon service...");

        // Reload systemd
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();

        // Enable service
        let output = std::process::Command::new("systemctl")
            .args(["--user", "enable", "rspmd"])
            .output()?;

        if output.status.success() {
            println!(
                "{}",
                colored::Colorize::green("Service installed and enabled successfully!")
            );
            println!("\nTo start the service: systemctl --user start rspmd");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to enable service: {}", stderr).as_str())
            );
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub async fn uninstall_service() -> Result<()> {
    println!("Uninstalling RSPM daemon service...");

    let _output = std::process::Command::new("systemctl")
        .args(["--user", "stop", "rspmd"])
        .output()?;

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "rspmd"])
        .output()?;

    let service_path = dirs::home_dir()
        .map(|p| p.join(".config/systemd/user/rspmd.service"))
        .unwrap_or_else(|| std::path::PathBuf::from(".config/systemd/user/rspmd.service"));

    if service_path.exists() {
        std::fs::remove_file(&service_path)?;
    }

    println!(
        "{}",
        colored::Colorize::green("Service uninstalled successfully!")
    );

    Ok(())
}

#[cfg(target_os = "macos")]
pub async fn install_service(output: bool) -> Result<()> {
    use std::path::PathBuf;

    let rspm_dir = dirs::home_dir()
        .map(|p| p.join(".rspm"))
        .unwrap_or_else(|| PathBuf::from(".rspm"));

    let rspmd_path = std::env::current_exe()
        .map(|p| p.parent().map(|p| p.join("rspmd")).unwrap_or_default())
        .unwrap_or_else(|_| PathBuf::from("rspmd"));

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.rspm.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>WorkingDirectory</key>
    <string>{}</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>{}/logs/daemon-out.log</string>
    <key>StandardErrorPath</key>
    <string>{}/logs/daemon-err.log</string>
    <key>ProcessType</key>
    <string>Background</string>
    <key>ThrottleInterval</key>
    <integer>10</integer>
    <key>Description</key>
    <string>RSPM Process Manager - Rust Process Manager Daemon</string>
</dict>
</plist>
"#,
        rspmd_path.display(),
        rspm_dir.display(),
        rspm_dir.display(),
        rspm_dir.display()
    );

    if output {
        println!("{}", plist_content);
        println!("\n# To install, run:");
        println!("# cp com.rspm.daemon.plist ~/Library/LaunchAgents/");
        println!("# launchctl load ~/Library/LaunchAgents/com.rspm.daemon.plist");
    } else {
        let plist_dir = dirs::home_dir()
            .map(|p| p.join("Library/LaunchAgents"))
            .unwrap_or_else(|| PathBuf::from("Library/LaunchAgents"));

        std::fs::create_dir_all(&plist_dir)?;
        let plist_path = plist_dir.join("com.rspm.daemon.plist");

        std::fs::write(&plist_path, &plist_content)?;

        println!("Installing RSPM daemon service...");

        let output = std::process::Command::new("launchctl")
            .args(["load", &plist_path.display().to_string()])
            .output()?;

        if output.status.success() {
            println!(
                "{}",
                colored::Colorize::green("Service installed successfully!")
            );
            println!("\nTo start the service: launchctl start com.rspm.daemon");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to install service: {}", stderr).as_str())
            );
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub async fn uninstall_service() -> Result<()> {
    println!("Uninstalling RSPM daemon service...");

    let plist_path = dirs::home_dir()
        .map(|p| p.join("Library/LaunchAgents/com.rspm.daemon.plist"))
        .unwrap_or_else(|| std::path::PathBuf::from("Library/LaunchAgents/com.rspm.daemon.plist"));

    let _ = std::process::Command::new("launchctl")
        .args(["unload", &plist_path.display().to_string()])
        .output();

    if plist_path.exists() {
        std::fs::remove_file(&plist_path)?;
    }

    println!(
        "{}",
        colored::Colorize::green("Service uninstalled successfully!")
    );

    Ok(())
}

// Fallback for unsupported platforms
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub async fn install_service(output: bool) -> Result<()> {
    if output {
        println!("Service installation is not supported on this platform.");
        println!("Please manually configure your service manager.");
    } else {
        return Err(rspm_common::RspmError::InternalError(
            "Service installation is not supported on this platform.".to_string(),
        ));
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub async fn uninstall_service() -> Result<()> {
    return Err(rspm_common::RspmError::InternalError(
        "Service uninstallation is not supported on this platform.".to_string(),
    ));
}
