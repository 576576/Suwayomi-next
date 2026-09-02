@echo off
REM ============================================================
REM  Suwayomi (next) - manual build producing the base artifact
REM  (no bundled JRE, no Electron) — naming matches the CI suffix
REM  convention: an un-suffixed name means neither bundle.
REM  CI variants: ...+jre / ...+electron(+jre) via release.yml.
REM
REM  Output: target\artifacts\  (cleared on every run)
REM    Suwayomi-r{code}-windows-x64\           unpacked stage
REM    Suwayomi-r{code}-windows-x64.zip        release zip (base)
REM
REM  Requires: cargo (Rust), JDK 17+ (jvm-sandbox jar),
REM  git, curl, python (zip extraction), PowerShell (zip pack)
REM ============================================================
setlocal
cd /d "%~dp0"

REM ---- version, identical to CI: commit count + 3000 -> r{code} ----
for /f %%c in ('git rev-list --count HEAD') do set "COUNT=%%c"
set /a VER_CODE=COUNT+3000
set "VER=r%VER_CODE%"
set "ART=target\artifacts"
set "STAGE=%ART%\Suwayomi-%VER%-windows-x64"
echo version: %VER% (code %VER_CODE%, commits %COUNT%)

REM ---- [0/6] clean artifacts ----
echo [0/6] cleaning %ART% ...
if exist "%ART%" rmdir /s /q "%ART%"
mkdir "%ART%"

REM [1/6] headless server
echo [1/6] Building suwayomi-server (release) ...
cargo build --release -p suwayomi-server
if errorlevel 1 goto :error

REM [2/6] tray shell
echo [2/6] Building tray shell suwayomi.exe (release) ...
cargo build --release --manifest-path tauri-app\Cargo.toml
if errorlevel 1 goto :error

REM [3/6] jvm-sandbox fat jar
echo [3/6] Building jvm-sandbox jar ...
pushd jvm-sandbox
call gradlew -q jar --no-daemon
if errorlevel 1 (
  popd
  goto :error
)
popd

REM [4/6] assemble stage layout (matches CI: exe top-level, server+jar in bin/)
echo [4/6] Assembling %STAGE% ...
mkdir "%STAGE%\bin"
copy /y tauri-app\target\release\suwayomi.exe "%STAGE%\" >nul
copy /y target\release\suwayomi-server.exe "%STAGE%\bin\" >nul
copy /y jvm-sandbox\build\libs\suwayomi-jvm-sandbox.jar "%STAGE%\bin\jvm-sandbox.jar" >nul

REM [5/6] bundle WebUI (fork latest release) + data dirs
echo [5/6] Downloading WebUI (fork latest release) ...
for /f "usebackq delims=" %%u in (`curl -s "https://api.github.com/repos/576576/Suwayomi-WebUI/releases/latest" ^| python -c "import json,sys;d=json.load(sys.stdin);print([a['browser_download_url'] for a in d.get('assets',[]) if a['name'].endswith('.zip')][0])"`) do set "WEBUI_ZIP=%%u"
if "%WEBUI_ZIP%"=="" (
  echo ERROR: failed to resolve WebUI release asset.
  goto :error
)
curl -sL -o "%ART%\webui.zip" "%WEBUI_ZIP%"
if errorlevel 1 goto :error
python scripts\unzip_any.py "%ART%\webui.zip" "%STAGE%\webui"
if errorlevel 1 goto :error
del /q "%ART%\webui.zip"
mkdir "%STAGE%\data\autobackup" "%STAGE%\data\downloads" "%STAGE%\data\local"

REM [6/6] pack zip (same naming as CI)
echo [6/6] Compressing Suwayomi-%VER%-windows-x64.zip ...
powershell -Command "Compress-Archive -Path '%STAGE%' -DestinationPath '%ART%\Suwayomi-%VER%-windows-x64.zip' -Force"
if errorlevel 1 goto :error

echo.
echo ============================================================
echo  Build OK. Artifacts:
echo    %~dp0%ART%\Suwayomi-%VER%-windows-x64.zip
echo    %~dp0%ART%\Suwayomi-%VER%-windows-x64\  (unpacked)
echo ============================================================
exit /b 0

:error
echo.
echo  Build FAILED (errorlevel %errorlevel%).
exit /b 1
