@echo off
:: Windows Service Installation Script for RSPM Daemon (rspmd)
:: Requires NSSM (Non-Sucking Service Manager)
:: Download: https://nssm.cc/download

setlocal enabledelayedexpansion

set "RSPM_DIR=%USERPROFILE%\.rspm"
set "RSPMD_EXE="
set "NSSM_PATH=nssm"

:: Detect rspmd.exe path
if exist "%~dp0rspmd.exe" (
    set "RSPMD_EXE=%~dp0rspmd.exe"
) else if exist "%~dp0target\release\rspmd.exe" (
    set "RSPMD_EXE=%~dp0target\release\rspmd.exe"
) else if exist "%~dp0target\debug\rspmd.exe" (
    set "RSPMD_EXE=%~dp0target\debug\rspmd.exe"
) else (
    echo Error: rspmd.exe not found in current directory
    echo Please place this script next to rspmd.exe or build the project first
    exit /b 1
)

echo ========================================
echo   RSPM Daemon Service Installation
echo ========================================
echo.
echo RSPM Directory: %RSPM_DIR%
echo RSPMD Executable: %RSPMD_EXE%
echo.

:: Check if NSSM is available
where %NSSM_PATH% >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo NSSM not found in PATH
    echo Please install NSSM first: https://nssm.cc/download
    echo Or place nssm.exe in the same directory as this script
    set "NSSM_PATH=%~dp0nssm.exe"
    if not exist "%NSSM_PATH%" (
        exit /b 1
    )
)

echo Installing RSPM daemon as Windows service...

:: Create RSPM directories if not exist
if not exist "%RSPM_DIR%" mkdir "%RSPM_DIR%"
if not exist "%RSPM_DIR%\db" mkdir "%RSPM_DIR%\db"
if not exist "%RSPM_DIR%\logs" mkdir "%RSPM_DIR%\logs"
if not exist "%RSPM_DIR%\pid" mkdir "%RSPM_DIR%\pid"
if not exist "%RSPM_DIR%\sock" mkdir "%RSPM_DIR%\sock"

:: Install service
"%NSSM_PATH%" install rspmd "%RSPMD_EXE%"
if %ERRORLEVEL% neq 0 (
    echo Failed to install service
    exit /b 1
)

:: Configure service
"%NSSM_PATH%" set rspmd AppDirectory "%RSPM_DIR%"
"%NSSM_PATH%" set rspmd DisplayName "RSPM Process Manager"
"%NSSM_PATH%" set rspmd Description "Rust Process Manager - Process management daemon"
"%NSSM_PATH%" set rspmd Start SERVICE_AUTO_START
"%NSSM_PATH%" set rspmd ObjectName LocalSystem
"%NSSM_PATH%" set rspmd Type SERVICE_WIN32_OWN_PROCESS
"%NSSM_PATH%" set rspmd AppStdout "%RSPM_DIR%\logs\daemon-out.log"
"%NSSM_PATH%" set rspmd AppStderr "%RSPM_DIR%\logs\daemon-err.log"
"%NSSM_PATH%" set rspmd AppRotateFiles 1
"%NSSM_PATH%" set rspmd AppRotateBytes 10485760

echo.
echo Service installed successfully!
echo.
echo To start the service:
echo   net start rspmd
echo.
echo To stop the service:
echo   net stop rspmd
echo.
echo To uninstall:
echo   "%NSSM_PATH%" remove rspmd confirm
echo.

endlocal