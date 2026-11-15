import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import MiniApp from "./MiniApp";
import { initTheme } from "./theme";
import "./styles.css";

const label = getCurrentWebviewWindow().label;
// The mini (HudWindow) window needs a forced dark palette.
document.body.classList.add(`${label}-root`);
if (label === "main") initTheme();

const Root = label === "mini" ? <MiniApp /> : <App />;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{Root}</React.StrictMode>
);
