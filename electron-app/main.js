// Suwayomi WebUI 桌面壳（Electron）——由托盘（suwayomi.exe）以无参方式启动，
// 通过 SUWAYOMI_WEBUI_URL 环境变量传入本地 server 地址，加载其 WebUI。
const { app, BrowserWindow } = require('electron');
const path = require('path');

// 托盘设置环境变量；兜底默认本地 8090（server 默认端口）。
const url = process.env.SUWAYOMI_WEBUI_URL || 'http://127.0.0.1:8090';

let mainWindow = null;

function createWindow() {
  mainWindow = new BrowserWindow({
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
  mainWindow.loadURL(url);
  mainWindow.on('closed', () => {
    mainWindow = null;
    app.quit();
  });
}

// 单实例兜底：托盘侧已做「已打开则聚焦」的去重，这里再兜一层，防止
// 直接双击 electron.exe 或托盘枚举失效时开出第二个窗口。
// 拿不到锁说明已经有一个实例在跑 → 立刻退出（已有实例会聚焦自己的窗口）。
if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (!mainWindow) {
      return;
    }

    if (mainWindow.isMinimized()) {
      mainWindow.restore();
    }

    mainWindow.focus();
  });

  app.whenReady().then(createWindow);
}

app.on('window-all-closed', () => app.quit());
