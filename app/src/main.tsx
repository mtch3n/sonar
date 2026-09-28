import React, { lazy, Suspense } from "react";
import ReactDOM from "react-dom/client";

// Each window loads only its own page, and so only its own styles: the Settings
// window's Tailwind would restyle the search bar.
const App = lazy(() => import("./App"));
const Settings = lazy(() => import("./Settings"));

// Match the system theme before the first paint; settings.toml can override it once loaded.
document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches
  ? "dark"
  : "light";

const settings = location.hash === "#settings";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Suspense>{settings ? <Settings /> : <App />}</Suspense>
  </React.StrictMode>,
);
