@echo off
REM ============================================================
REM  Suwayomi (next) - manual release build script (Windows)
REM
REM  Artifacts:
REM    target\release\suwayomi-server.exe        headless server
REM    tauri-app\target\release\suwayomi.exe      desktop tray shell
REM
REM  Optional: JVM extension sandbox (needs JDK 17+ and the
REM  AndroidCompat jar, see jvm-sandbox/README):
REM    gradle -p jvm-sandbox build
REM ============================================================
setlocal
cd /d "%~dp0"

echo [1/2] Building suwayomi-server (release) ...
cargo build --release -p suwayomi-server
if errorlevel 1 goto :error

echo [2/2] Building tray shell suwayomi.exe (release) ...
cargo build --release --manifest-path tauri-app\Cargo.toml
if errorlevel 1 goto :error

echo.
echo ============================================================
echo  Build OK. Artifacts:
echo    %~dp0target\release\suwayomi-server.exe
echo    %~dp0tauri-app\target\release\suwayomi.exe
echo ============================================================
exit /b 0

:error
echo.
echo  Build FAILED (errorlevel %errorlevel%).
exit /b 1
