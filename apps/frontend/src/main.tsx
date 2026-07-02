import { isDesktop, getPlatform } from "@/adapters";
import React from "react";
import ReactDOM from "react-dom/client";
import { debugAddonState, isAddonDevModeEnabled, loadAllAddons } from "./addons/addons-loader";
import "./addons/addons-runtime-context";
import App from "./App";
import "./globals.css";

if (isAddonDevModeEnabled) {
  void import("./addons/addons-dev-mode");
} else if (isDesktop && !import.meta.env.DEV) {
  // Only install lockdown on actual desktop platforms (not iOS/Android running in Tauri).
  // `isDesktop` is a compile-time constant that is true for ALL Tauri builds, so we
  // check the runtime platform to avoid disabling text selection and gestures on mobile.
  void getPlatform().then(async (platform) => {
    if (!platform.is_mobile) {
      const { installLockdown } = await import("./lockdown");
      installLockdown();
    }
  });
}

if (import.meta.env.DEV) {
  Object.defineProperty(globalThis, "debugAddons", {
    configurable: true,
    enumerable: false,
    value: debugAddonState,
    writable: false,
  });
}

// Load addons after context is injected
loadAllAddons();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
