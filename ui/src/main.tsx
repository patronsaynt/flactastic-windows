import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/inter";
import "@fontsource/jetbrains-mono/400.css";
import "./theme/tokens.css";
import { App } from "./App";
import { MiniPlayer } from "./features/mini/MiniPlayer";

// The tray mini-player loads the same bundle with `?view=mini`.
const mini = new URLSearchParams(location.search).get("view") === "mini";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{mini ? <MiniPlayer /> : <App />}</React.StrictMode>,
);
