/** One result. Its action runs in Sonar; the window only sends back the id. */
export type Row = {
  id: string;
  title: string;
  subtitle: string | null;
  meta: string | null;
  icon: string;
  image: string | null;
  action: string;
  alt: string | null;
};

/** The results from one source, like the file index or a plugin. */
export type Section = {
  key: string;
  title: string;
  rank: number;
  rows: Row[];
  message: string | null;
  warning: boolean;
  pending: boolean;
};

/** How the window looks, from settings.toml, and what Sonar wants to tell the user. */
export type View = {
  theme: "system" | "light" | "dark";
  accent: string;
  width: number;
  rows: number;
  indexing: boolean;
  notices: string[];
};

export type Outcome =
  | { then: "close" }
  | { then: "fill"; text: string }
  | { then: "refresh"; notice: string };

export type SettingValue = string | number | boolean;

/** `[plugins.<id>]`: Sonar's own keys, then the plugin's settings that differ from their defaults. */
export type PluginSettings = {
  enabled: boolean;
  keyword: string | null;
  [key: string]: SettingValue | null;
};

/** A setting a plugin declares; the Settings window draws a field for it. */
export type Setting = { key: string; title: string; description: string | null } & (
  | { type: "text"; default: string; placeholder: string | null }
  | { type: "number"; default: number; min: number | null; max: number | null }
  | { type: "toggle"; default: boolean }
  | { type: "choice"; default: string; options: { value: string; title: string }[] }
);

/** settings.toml, as the Settings window edits it. */
export type Settings = {
  shortcut: string;
  marketplaces: string[];
  appearance: { theme: "system" | "light" | "dark"; accent: string; width: number; rows: number };
  search: { limit: number };
  files: { editor: string; terminal: string };
  index: { rescan_minutes: number };
  updates: { check: boolean };
  plugins: Record<string, PluginSettings>;
};

export type PluginInfo = {
  id: string;
  name: string;
  description: string | null;
  keyword: string | null;
  image: string | null;
  icon: string;
  settings: Setting[];
};

/** An installed editor or terminal, and its command as settings.toml holds it. */
export type Tool = { name: string; command: string };

export type Editor = {
  settings: Settings;
  plugins: PluginInfo[];
  path: string;
  editors: Tool[];
  terminals: Tool[];
  problem: string | null;
};
