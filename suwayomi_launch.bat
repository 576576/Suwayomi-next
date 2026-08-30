@echo off
setlocal
chcp 65001 >nul
cd /d "%~dp0"

rem ============================================================
rem  Suwayomi 启动脚本：先拉起无头服务器，等待就绪后打开 WebUI
rem  发布布局：suwayomi.exe + suwayomi_launch.bat + webui/ + data/
rem ============================================================

set "PORT=8090"
if defined SUWAYOMI_PORT set "PORT=%SUWAYOMI_PORT%"

rem 工作数据目录（不存在时创建）
if not exist "data\autobackup" mkdir "data\autobackup"
if not exist "data\downloads"  mkdir "data\downloads"
if not exist "data\local"      mkdir "data\local"

echo [suwayomi] 启动服务器 (127.0.0.1:%PORT%) ...
rem 隐藏窗口启动（start /min 会残留最小化 cmd 窗口）
powershell -NoProfile -Command "Start-Process -FilePath '%~dp0suwayomi.exe' -WorkingDirectory '%~dp0' -WindowStyle Hidden"

echo [suwayomi] 等待服务器就绪...
:wait
timeout /t 1 /nobreak >nul
powershell -NoProfile -Command "try { $c = New-Object Net.Sockets.TcpClient; $c.Connect('127.0.0.1', %PORT%); $c.Close(); exit 0 } catch { exit 1 }" >nul 2>&1
if errorlevel 1 goto wait

echo [suwayomi] 服务器就绪，打开 WebUI ...
start "" "http://127.0.0.1:%PORT%"
endlocal
