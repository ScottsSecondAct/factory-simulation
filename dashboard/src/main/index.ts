import { app, BrowserWindow, ipcMain, nativeImage } from "electron";
import { join } from "path";
import { existsSync } from "fs";
import { SimManager } from "./sim";

let mainWindow: BrowserWindow | null = null;
let splashWindow: BrowserWindow | null = null;
const sim = new SimManager();

const SPLASH_DURATION_MS = 5000;

function resourcePath(name: string): string {
  // In production (packaged): resources are in extraResources
  const prodPath = join(process.resourcesPath, "resources", name);
  if (existsSync(prodPath)) return prodPath;
  // In dev: resources directory is at project root
  return join(__dirname, "../../resources", name);
}

function getAppIcon(): Electron.NativeImage | undefined {
  if (process.platform === "win32") {
    return nativeImage.createFromPath(resourcePath("icon.ico"));
  }
  return nativeImage.createFromPath(resourcePath("icon.png"));
}

function createSplashWindow(): void {
  const icon = getAppIcon();
  splashWindow = new BrowserWindow({
    width: 720,
    height: 405,
    frame: false,
    resizable: false,
    transparent: false,
    center: true,
    skipTaskbar: false,
    backgroundColor: "#101820",
    icon,
    show: false,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  splashWindow.loadFile(resourcePath("splash/splash.html"));
  splashWindow.once("ready-to-show", () => {
    splashWindow?.show();
  });
}

function createWindow(): void {
  const icon = getAppIcon();
  mainWindow = new BrowserWindow({
    width: 1400,
    height: 900,
    minWidth: 1024,
    minHeight: 700,
    backgroundColor: "#1b2033",
    icon,
    show: false,
    webPreferences: {
      preload: join(__dirname, "../preload/index.js"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  if (process.env.ELECTRON_RENDERER_URL) {
    mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL);
  } else {
    mainWindow.loadFile(join(__dirname, "../renderer/index.html"));
  }

}

function showMainAfterSplash(): void {
  let mainReady = false;
  let timerDone = false;

  const tryShow = (): void => {
    if (mainReady && timerDone) {
      splashWindow?.close();
      splashWindow = null;
      mainWindow?.show();
    }
  };

  mainWindow?.once("ready-to-show", () => {
    mainReady = true;
    tryShow();
  });

  setTimeout(() => {
    timerDone = true;
    tryShow();
  }, SPLASH_DURATION_MS);
}

app.whenReady().then(() => {
  if (process.platform === "darwin" && app.dock) {
    const dockIcon = nativeImage.createFromPath(resourcePath("icon.png"));
    if (!dockIcon.isEmpty()) app.dock.setIcon(dockIcon);
  }
  createSplashWindow();
  createWindow();
  showMainAfterSplash();
});

app.on("window-all-closed", () => {
  sim.kill();
  app.quit();
});

sim.on("message", (msg: Record<string, unknown>) => {
  mainWindow?.webContents.send("sim:message", msg);
});

sim.on("stderr", (line: string) => {
  mainWindow?.webContents.send("sim:stderr", line);
});

sim.on("exit", (code: number | null, lastStderr: string[]) => {
  mainWindow?.webContents.send("sim:exit", { code, lastStderr });
});

sim.on("error", (msg: string) => {
  mainWindow?.webContents.send("sim:error", msg);
});

ipcMain.handle("sim:start", (_event, opts) => {
  try {
    sim.start(opts);
  } catch (e: unknown) {
    return { error: (e as Error).message };
  }
  return { ok: true };
});

ipcMain.on("sim:command", (_event, cmd) => {
  sim.send(cmd);
});

ipcMain.handle("sim:kill", () => {
  sim.kill();
});
