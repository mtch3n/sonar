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

export type PluginSettings = { enabled: boolean; keyword: string | null };

/** settings.toml, as the Settings window edits it. */
export type Settings = {
  shortcut: string;
  marketplaces: string[];
  appearance: { theme: "system" | "light" | "dark"; accent: string; width: number; rows: number };
  search: { limit: number };
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
};

export type Editor = {
  settings: Settings;
  plugins: PluginInfo[];
  path: string;
  problem: string | null;
};
