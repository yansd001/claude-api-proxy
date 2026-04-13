@echo off
chcp 65001 >nul 2>&1

set "ROOT=%~dp0"
set "FRONTEND=%ROOT%frontend"
set "BACKEND_RUST=%ROOT%backend-rust"
set "PUBLISH=%ROOT%publish"

echo ============================================================
echo   Claude API Proxy (Rust) - Build and Publish
echo ============================================================
echo.

:: ----------------------------------------------------------------
:: Step 0: Check dependencies
:: ----------------------------------------------------------------
echo [0/4] Checking dependencies...

where npm.cmd >nul 2>&1
if %errorlevel% neq 0 (
    echo [Error] npm not found. Please install Node.js.
    pause
    exit /b 1
)

where cargo.exe >nul 2>&1
if %errorlevel% neq 0 (
    echo [Error] cargo not found. Please install Rust.
    pause
    exit /b 1
)

echo       npm:    OK
echo       cargo:  OK
echo.

:: ----------------------------------------------------------------
:: Step 1: Build Vue frontend
:: ----------------------------------------------------------------
echo [1/4] Building Vue frontend...
cd /d "%FRONTEND%"

call npm install
if %errorlevel% neq 0 (
    echo [Error] npm install failed.
    pause
    exit /b 1
)

call npm run build
if %errorlevel% neq 0 (
    echo [Error] npm run build failed.
    pause
    exit /b 1
)
echo.

:: ----------------------------------------------------------------
:: Step 2: Copy frontend dist to publish\static
:: ----------------------------------------------------------------
echo [2/4] Preparing output directory...
cd /d "%ROOT%"

if exist "%PUBLISH%" (
    rmdir /s /q "%PUBLISH%"
)
mkdir "%PUBLISH%"

xcopy /e /i /q "%FRONTEND%\dist" "%PUBLISH%\static"
if %errorlevel% neq 0 (
    echo [Error] Failed to copy frontend files.
    pause
    exit /b 1
)
echo       frontend\dist  ->  publish\static
echo.

:: ----------------------------------------------------------------
:: Step 3: Build Rust backend
:: ----------------------------------------------------------------
echo [3/4] Building Rust backend (release)...
cd /d "%BACKEND_RUST%"

cargo build --release
if %errorlevel% neq 0 (
    echo [Error] cargo build failed
    pause
    exit /b 1
)
echo.

:: ----------------------------------------------------------------
:: Step 4: Copy binary to publish
:: ----------------------------------------------------------------
echo [4/4] Copying binary to publish...
cd /d "%ROOT%"

copy "%BACKEND_RUST%\target\release\claude-api-proxy.exe" "%PUBLISH%\claude-api-proxy.exe"
if %errorlevel% neq 0 (
    echo [Error] Failed to copy binary.
    pause
    exit /b 1
)

echo.
echo ============================================================
echo   Build complete!
echo   Output:  %PUBLISH%
echo   Exe:     %PUBLISH%\claude-api-proxy.exe
echo   UI:      %PUBLISH%\static\
echo   config.json will be created next to the exe on first launch.
echo ============================================================
echo.
pause
