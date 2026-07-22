import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

// The scheduler's toast is a transparent Tauri window (design spec §3.1) — its
// page must stay see-through so only the floating card shows. Every other
// window (Settings, Stats, Custom pause, and the dev demo) is an ordinary
// opaque window, flagged here so the stylesheet can paint it a solid backdrop.
const view = new URLSearchParams(window.location.search).get("view");
if (view !== "toast") {
  document.documentElement.dataset.opaque = "true";
}

const target = document.getElementById("app");
if (!target) {
  throw new Error("Missing #app mount point in index.html");
}

const app = mount(App, { target });

export default app;
