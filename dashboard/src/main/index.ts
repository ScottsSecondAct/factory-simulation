import { app, BrowserWindow, ipcMain } from "electron";
import { join } from "path";
import { SimManager } from "./sim";

let mainWindow: BrowserWindow | null = null;
const sim = new SimManager();

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1400,
    height: 900,
    minWidth: 1024,
    minHeight: 700,
    backgroundColor: "#0f1117",
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

app.whenReady().then(createWindow);
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
