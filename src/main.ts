import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import "./styles/tokens.css";
import App from "./App.svelte";

function logFrontend(message: string) {
  invoke("frontend_log", { message }).catch(() => {});
}

logFrontend("main.ts starting");

window.addEventListener("error", (event) => {
  logFrontend(`error ${event.message} at ${event.filename}:${event.lineno}:${event.colno}`);
});

window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason instanceof Error ? event.reason.stack || event.reason.message : String(event.reason);
  logFrontend(`unhandledrejection ${reason}`);
});

const app = mount(App, { target: document.getElementById("app")! });

logFrontend("svelte mounted");

export default app;
