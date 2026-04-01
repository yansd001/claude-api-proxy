@echo off
setlocal enabledelayedexpansion
chcp 65001 >nul 2>&1

set "ROOT=%~dp0"
set "FRONTEND=%ROOT%frontend"
set "BACKEND=%ROOT%backend"
set "PUBLISH=%ROOT%publish"
set PYTHONUTF8=1

echo ============================================================
echo   Claude API Proxy - Build and Publish
echo ============================================================
echo.

:: ----------------------------------------------------------------
:: Step 0: Check dependencies
:: ----------------------------------------------------------------
echo [0/4] Checking dependencies...

where npm >nul 2>&1
if errorlevel 1 (
    echo [Error] npm not found. Please install Node.js.
    pause
    exit /b 1
)

where python >nul 2>&1
if errorlevel 1 (
    where py >nul 2>&1
    if errorlevel 1 (
        echo [Error] Python not found. Please install Python 3.10+
        pause
        exit /b 1
    )
    set PYTHON_CMD=py
) else (
    set PYTHON_CMD=python
)

echo       npm:    OK
echo       python: OK
echo.

:: ----------------------------------------------------------------
:: Step 1: Build Vue frontend
:: ----------------------------------------------------------------
echo [1/4] Building Vue frontend...
cd /d "%FRONTEND%"

call npm install --frozen-lockfile
if errorlevel 1 (
    echo [Error] npm install failed.
    pause
    exit /b 1
)

call npm run build
if errorlevel 1 (
    echo [Error] npm run build failed.
    pause
    exit /b 1
)
echo.

:: ----------------------------------------------------------------
:: Step 2: Copy frontend dist to backend/static
:: ----------------------------------------------------------------
echo [2/4] Copying frontend dist to backend/static...
cd /d "%ROOT%"

if exist "%BACKEND%\static" (
    rmdir /s /q "%BACKEND%\static"
)
xcopy /e /i /q "%FRONTEND%\dist" "%BACKEND%\static"
if errorlevel 1 (
    echo [Error] Failed to copy frontend files.
    pause
    exit /b 1
)
echo       frontend\dist  ->  backend\static
echo.

:: ----------------------------------------------------------------
:: Step 3: Install Python deps and package with PyInstaller
:: ----------------------------------------------------------------
echo [3/4] Installing Python dependencies and packaging exe...
cd /d "%BACKEND%"

%PYTHON_CMD% -m pip install -r requirements.txt -r build-requirements.txt -q
if errorlevel 1 (
    echo [Error] pip install failed
    pause
    exit /b 1
)

:: Clean publish dir before packaging so PyInstaller outputs directly into it
if exist "%PUBLISH%" (
    rmdir /s /q "%PUBLISH%"
)
mkdir "%PUBLISH%"

%PYTHON_CMD% -m PyInstaller ^
    --onefile ^
    --name claude-api-proxy ^
    --add-data "static;static" ^
    --collect-all uvicorn ^
    --collect-all fastapi ^
    --collect-all starlette ^
    --collect-all httpx ^
    --hidden-import anyio._backends._asyncio ^
    --hidden-import anyio._backends._trio ^
    --distpath "%PUBLISH%" ^
    --clean ^
    main.py

if errorlevel 1 (
    echo [Error] PyInstaller failed
    pause
    exit /b 1
)
echo.

:: ----------------------------------------------------------------
:: Step 4: Copy frontend static files to publish\static
:: ----------------------------------------------------------------
echo [4/4] Copying frontend static files to publish\static...
cd /d "%ROOT%"

xcopy /e /i /q "%BACKEND%\static" "%PUBLISH%\static"
if errorlevel 1 (
    echo [Error] Failed to copy static files.
    pause
    exit /b 1
)

echo.
echo ============================================================
echo   Build complete!
echo   Output:  %PUBLISH%
echo   Exe:     %PUBLISH%\claude-api-proxy.exe
echo   UI:      %PUBLISH%\static\  (can be updated independently)
echo   config.json will be created next to the exe on first launch.
echo ============================================================
echo.
pause
