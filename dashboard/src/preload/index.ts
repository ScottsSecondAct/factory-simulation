import { contextBridge, ipcRenderer, IpcRendererEvent } from "electron";

contextBridge.exposeInMainWorld("simBridge", {
  start: (opts?: Record<string, unknown>) =>
    ipcRenderer.invoke("sim:start", opts),
  command: (cmd: Record<string, unknown>) =>
    ipcRenderer.send("sim:command", cmd),
  kill: () => ipcRenderer.invoke("sim:kill"),

  onMessage: (cb: (msg: unknown) => void) => {
    const handler = (_e: IpcRendererEvent, msg: unknown) => cb(msg);
    ipcRenderer.on("sim:message", handler);
    return () => ipcRenderer.removeListener("sim:message", handler);
  },
  onStderr: (cb: (line: string) => void) => {
    const handler = (_e: IpcRendererEvent, line: string) => cb(line);
    ipcRenderer.on("sim:stderr", handler);
    return () => ipcRenderer.removeListener("sim:stderr", handler);
  },
  onExit: (cb: (data: { code: number | null; lastStderr: string[] }) => void) => {
    const handler = (_e: IpcRendererEvent, data: { code: number | null; lastStderr: string[] }) => cb(data);
    ipcRenderer.on("sim:exit", handler);
    return () => ipcRenderer.removeListener("sim:exit", handler);
  },
  onError: (cb: (msg: string) => void) => {
    const handler = (_e: IpcRendererEvent, msg: string) => cb(msg);
    ipcRenderer.on("sim:error", handler);
    return () => ipcRenderer.removeListener("sim:error", handler);
  },
});
