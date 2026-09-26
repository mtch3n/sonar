/** Applies the theme and accent from settings.toml, following the system when the
 * theme is "system". Returns a function that stops following it. */
export function applyLook(theme: "system" | "light" | "dark", accent: string): () => void {
  const root = document.documentElement;
  root.style.setProperty("--accent", accent);
  const dark = matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    root.dataset.theme = theme === "system" ? (dark.matches ? "dark" : "light") : theme;
  };
  apply();
  dark.addEventListener("change", apply);
  return () => dark.removeEventListener("change", apply);
}
