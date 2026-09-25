import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// Match the system theme before the first paint; settings.toml can override it once loaded.
document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches
  ? "dark"
  : "light";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
