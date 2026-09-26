import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Settings from "./Settings";

// Match the system theme before the first paint; settings.toml can override it once loaded.
document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches
  ? "dark"
  : "light";

const settings = location.hash === "#settings";
if (settings) document.documentElement.classList.add("page");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{settings ? <Settings /> : <App />}</React.StrictMode>,
);
