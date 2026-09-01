// Suwayomi WebUI 桌面壳（Electron）——由托盘（suwayomi.exe）以无参方式启动，
// 通过 SUWAYOMI_WEBUI_URL 环境变量传入本地 server 地址，加载其 WebUI。
const { app, BrowserWindow } = require('electron');
const path = require('path');

// 托盘设置环境变量；兜底默认本地 8090（server 默认端口）。
const url = process.env.SUWAYOMI_WEBUI_URL || 'http://127.0.0.1:8090';

function createWindow() {
  const win = new BrowserWindow({
    width: 1280,
    height: 860,
    autoHideMenuBar: true,
    title: 'Suwayomi',
    backgroundColor: '#0d1117',
    // 打包时 icon.ico 与 main.js 一起放入 resources/app/（见 release-alpha.yml）；
    // Windows 标题栏/任务栏图标由此指定，替代 Electron 默认图标。
    icon: path.join(__dirname, 'icon.ico'),
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  win.loadURL(url);
  win.on('closed', () => app.quit());
}

app.whenReady().then(createWindow);
app.on('window-all-closed', () => app.quit());
