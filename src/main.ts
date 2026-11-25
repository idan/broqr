import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { createApp } from "./app";
import { nativeBackend } from "./backend";
import "./style.css";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("Missing app root.");
const desktop = isTauri();
const app = createApp(root, nativeBackend, desktop);
let unlisten: (() => void) | undefined;
let disposed = false;

if (desktop) {
  void getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "enter" || event.payload.type === "over") app.setDropActive(true);
    else app.setDropActive(false);
    if (event.payload.type === "drop") void app.receivePaths(event.payload.paths);
  }).then((remove) => {
    if (disposed) remove();
    else unlisten = remove;
  });
}

window.addEventListener("beforeunload", () => {
  disposed = true;
  unlisten?.();
  app.dispose();
});
