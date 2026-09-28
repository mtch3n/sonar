import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronRight, Minus, TriangleAlert, X } from "lucide-react";
import { Fragment, type KeyboardEvent, type ReactNode, useEffect, useState } from "react";
import { Glyph } from "./icons";
import { applyLook } from "./look";
import { hint, withValue } from "./pluginSettings";
import type { Editor, PluginSettings, Setting, SettingValue, Tool, Settings as Values } from "./types";
import "./styles.css";
import "./settings.css";

const isMac = navigator.userAgent.includes("Mac");
const platform = isMac ? "mac" : navigator.userAgent.includes("Windows") ? "windows" : "linux";

export default function Settings() {
  const [editor, setEditor] = useState<Editor | null>(null);
  const [draft, setDraft] = useState<Values | null>(null);
  const [saved, setSaved] = useState<Values | null>(null);
  const [status, setStatus] = useState<{ text: string; error: boolean } | null>(null);
  const [market, setMarket] = useState("");
  const [expanded, setExpanded] = useState<string[]>([]);

  useEffect(() => {
    document.documentElement.dataset.platform = platform;
    invoke<Editor>("settings_get").then((loaded) => {
      setEditor(loaded);
      setDraft(loaded.settings);
      setSaved(loaded.settings);
    });
  }, []);

  useEffect(() => {
    if (saved) return applyLook(saved.appearance.theme, saved.appearance.accent);
  }, [saved]);

  if (!editor || !draft || !saved) return null;

  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);
  const change = (next: Values) => {
    setDraft(next);
    setStatus(null);
  };
  const appearance = (patch: Partial<Values["appearance"]>) =>
    change({ ...draft, appearance: { ...draft.appearance, ...patch } });
  const own = (id: string): PluginSettings => draft.plugins[id] ?? { enabled: true, keyword: null };
  const plugin = (id: string, patch: { enabled?: boolean; keyword?: string | null }) =>
    change({ ...draft, plugins: { ...draft.plugins, [id]: { ...own(id), ...patch } } });
  const setValue = (id: string, setting: Setting, value: SettingValue) =>
    change({ ...draft, plugins: { ...draft.plugins, [id]: withValue(own(id), setting, value) } });
  const toggleExpanded = (id: string) =>
    setExpanded(expanded.includes(id) ? expanded.filter((open) => open !== id) : [...expanded, id]);

  async function save() {
    if (!draft) return;
    try {
      await invoke("settings_save", { settings: draft });
      setSaved(draft);
      setStatus({ text: "Saved", error: false });
    } catch (err) {
      setStatus({ text: String(err), error: true });
    }
  }

  function addMarket() {
    const repo = market.trim();
    if (!repo || !draft) return;
    change({ ...draft, marketplaces: [...draft.marketplaces, repo] });
    setMarket("");
  }

  return (
    <div className="page">
      <TitleBar />
      <main className="settings">
        {editor.problem && (
          <p className="banner">
            <TriangleAlert size={16} strokeWidth={2} aria-hidden />
            settings.toml {editor.problem}. The form shows the last settings that worked. Saving
            fixes the file and keeps the old one as settings.toml.bak.
          </p>
        )}

        <Group title="Search bar">
          <Row label="Shortcut" hint="Click, then press the keys you want">
            <ShortcutField value={draft.shortcut} onChange={(shortcut) => change({ ...draft, shortcut })} />
          </Row>
          <Row label="Theme">
            <div className="segments" role="radiogroup" aria-label="Theme">
              {(["system", "light", "dark"] as const).map((theme) => (
                <button
                  key={theme}
                  type="button"
                  role="radio"
                  aria-checked={draft.appearance.theme === theme}
                  onClick={() => appearance({ theme })}
                >
                  {theme[0].toUpperCase() + theme.slice(1)}
                </button>
              ))}
            </div>
          </Row>
          <Row label="Accent color" hint="The selected result's icon and the text cursor">
            <div className="color">
              <input
                type="color"
                value={expand(draft.appearance.accent)}
                onChange={(e) => appearance({ accent: e.target.value })}
                aria-label="Accent color"
              />
              <input
                className="text short"
                value={draft.appearance.accent}
                onChange={(e) => appearance({ accent: e.target.value })}
                spellCheck={false}
              />
            </div>
          </Row>
          <Row label="Width" hint="480 to 1600">
            <NumberField value={draft.appearance.width} unit="px" onChange={(width) => appearance({ width })} />
          </Row>
          <Row label="Visible results" hint="How many show before the list scrolls, 3 to 20">
            <NumberField value={draft.appearance.rows} onChange={(rows) => appearance({ rows })} />
          </Row>
        </Group>

        <Group title="Search">
          <Row label="Results to find" hint="When the search has no limit: filter">
            <NumberField value={draft.search.limit} onChange={(limit) => change({ ...draft, search: { limit } })} />
          </Row>
          <Row label="Rescan everything every" hint="Changes show up within seconds; this catches any that slip by">
            <NumberField
              value={draft.index.rescan_minutes}
              unit="min"
              onChange={(rescan_minutes) => change({ ...draft, index: { rescan_minutes } })}
            />
          </Row>
          <Row label="Check for updates" hint="Look for new versions of Sonar on GitHub">
            <Toggle
              label="Check for updates"
              on={draft.updates.check}
              onChange={(check) => change({ ...draft, updates: { check } })}
            />
          </Row>
        </Group>

        <Group title="Files">
          <Row label="Code editor" hint="Opens projects, code, scripts and config files">
            <CommandField
              label="Code editor"
              value={draft.files.editor}
              tools={editor.editors}
              none="Each file's default app"
              onChange={(command) => change({ ...draft, files: { ...draft.files, editor: command } })}
            />
          </Row>
          <Row label="Terminal" hint="Opens folders">
            <CommandField
              label="Terminal"
              value={draft.files.terminal}
              tools={editor.terminals}
              none="The first one found"
              onChange={(command) => change({ ...draft, files: { ...draft.files, terminal: command } })}
            />
          </Row>
        </Group>

        <Group title="Plugins" note="Type plugins and a space in the search bar to install more.">
          {editor.plugins.map((info) => {
            const values = own(info.id);
            const open = expanded.includes(info.id);
            return (
              <Fragment key={info.id}>
                <div className="item">
                  <Glyph icon={info.icon} image={info.image} />
                  <span className="item-text">
                    <span className="item-title">{info.name}</span>
                    {info.description && <span className="item-hint">{info.description}</span>}
                  </span>
                  <input
                    className="text keyword"
                    value={values.keyword ?? ""}
                    placeholder={info.keyword ?? "No keyword"}
                    onChange={(e) => plugin(info.id, { keyword: e.target.value.trim() || null })}
                    aria-label={`Keyword for ${info.name}`}
                    spellCheck={false}
                  />
                  <Toggle
                    label={`Use ${info.name}`}
                    on={values.enabled}
                    onChange={(enabled) => plugin(info.id, { enabled })}
                  />
                  {info.settings.length > 0 ? (
                    <button
                      type="button"
                      className="icon-button disclosure"
                      aria-expanded={open}
                      aria-controls={`settings-${info.id}`}
                      aria-label={`${info.name} settings`}
                      onClick={() => toggleExpanded(info.id)}
                    >
                      <ChevronRight size={16} strokeWidth={2} aria-hidden />
                    </button>
                  ) : (
                    <span className="disclosure-space" />
                  )}
                </div>
                {open && (
                  <div id={`settings-${info.id}`} className="nested" aria-label={`${info.name} settings`}>
                    {info.settings.map((setting) => (
                      <Row key={setting.key} label={setting.title} hint={hint(setting)}>
                        <SettingField
                          setting={setting}
                          value={values[setting.key] ?? setting.default}
                          onChange={(value) => setValue(info.id, setting, value)}
                        />
                      </Row>
                    ))}
                  </div>
                )}
              </Fragment>
            );
          })}
        </Group>

        <Group title="Marketplaces" note="GitHub repositories whose plugins you can install.">
          {draft.marketplaces.map((repo) => (
            <div key={repo} className="item">
              <span className="item-text">
                <span className="item-title">{repo}</span>
              </span>
              <button
                type="button"
                className="icon-button"
                aria-label={`Remove ${repo}`}
                onClick={() =>
                  change({ ...draft, marketplaces: draft.marketplaces.filter((m) => m !== repo) })
                }
              >
                <X size={16} strokeWidth={2} aria-hidden />
              </button>
            </div>
          ))}
          <div className="item">
            <input
              className="text grow"
              value={market}
              placeholder="owner/name or a GitHub link"
              onChange={(e) => setMarket(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && addMarket()}
              aria-label="Marketplace to add"
              spellCheck={false}
            />
            <button type="button" className="button" onClick={addMarket} disabled={!market.trim()}>
              Add
            </button>
          </div>
        </Group>
      </main>

      <footer className="bar-footer">
        <button type="button" className="link" onClick={() => invoke("settings_open_file")}>
          Open settings.toml
        </button>
        <span className={status?.error ? "status error" : "status"} role="status">
          {status?.text}
        </span>
        <button type="button" className="button primary" onClick={save} disabled={!dirty}>
          Save
        </button>
      </footer>
    </div>
  );
}

/** The window's own title bar: drags the window, and has its buttons where the system's aren't drawn. */
function TitleBar() {
  const appWindow = getCurrentWindow();
  return (
    <header className="titlebar" data-tauri-drag-region>
      <h1 data-tauri-drag-region>Settings</h1>
      {!isMac && (
        <div className="window-buttons">
          <button type="button" className="icon-button" aria-label="Minimize" onClick={() => appWindow.minimize()}>
            <Minus size={16} strokeWidth={2} aria-hidden />
          </button>
          <button type="button" className="icon-button close" aria-label="Close" onClick={() => appWindow.close()}>
            <X size={16} strokeWidth={2} aria-hidden />
          </button>
        </div>
      )}
    </header>
  );
}

function Group({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="group" aria-label={title}>
      <h2>{title}</h2>
      <div className="list">{children}</div>
      {note && <p className="note">{note}</p>}
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="item">
      <span className="item-text">
        <span className="item-title">{label}</span>
        {hint && <span className="item-hint">{hint}</span>}
      </span>
      {children}
    </div>
  );
}

/** The field for a setting a plugin declares. */
function SettingField({
  setting,
  value,
  onChange,
}: {
  setting: Setting;
  value: SettingValue;
  onChange: (value: SettingValue) => void;
}) {
  switch (setting.type) {
    case "text":
      return (
        <input
          className="text field"
          value={String(value)}
          placeholder={setting.placeholder ?? ""}
          onChange={(e) => onChange(e.target.value)}
          aria-label={setting.title}
          spellCheck={false}
        />
      );
    case "number":
      return <NumberField value={Number(value)} label={setting.title} onChange={onChange} />;
    case "toggle":
      return <Toggle label={setting.title} on={value === true} onChange={onChange} />;
    case "choice":
      return (
        <select
          className="text field"
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
          aria-label={setting.title}
        >
          {setting.options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.title}
            </option>
          ))}
        </select>
      );
  }
}

/** Picks an installed app, or takes any command under "Other command…". */
function CommandField({
  label,
  value,
  tools,
  none,
  onChange,
}: {
  label: string;
  value: string;
  tools: Tool[];
  none: string;
  onChange: (command: string) => void;
}) {
  const listed = value === "" || tools.some((tool) => tool.command === value);
  const [other, setOther] = useState(!listed);
  return (
    <span className="command">
      <select
        className="text field"
        value={other ? OTHER : value}
        onChange={(e) => {
          const picked = e.target.value;
          setOther(picked === OTHER);
          if (picked !== OTHER) onChange(picked);
        }}
        aria-label={label}
      >
        <option value="">{none}</option>
        {tools.map((tool) => (
          <option key={tool.command} value={tool.command}>
            {tool.name}
          </option>
        ))}
        <option value={OTHER}>Other command…</option>
      </select>
      {other && (
        <input
          className="text field"
          value={value}
          placeholder="Command"
          onChange={(e) => onChange(e.target.value)}
          aria-label={`${label} command`}
          spellCheck={false}
          autoFocus
        />
      )}
    </span>
  );
}

/** The select value for "Other command…"; no real command starts with a NUL. */
const OTHER = "\u0000other";

function NumberField({
  value,
  unit,
  label,
  onChange,
}: {
  value: number;
  unit?: string;
  label?: string;
  onChange: (n: number) => void;
}) {
  return (
    <span className="number">
      <input
        className="text short"
        type="number"
        value={value}
        aria-label={label}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <span className="unit">{unit}</span>
    </span>
  );
}

function Toggle({ label, on, onChange }: { label: string; on: boolean; onChange: (on: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      className="switch"
      onClick={() => onChange(!on)}
    >
      <span />
    </button>
  );
}

/** Shows the shortcut the way this platform writes it, and records a new one. */
function ShortcutField({ value, onChange }: { value: string; onChange: (s: string) => void }) {
  const [recording, setRecording] = useState(false);

  function onKeyDown(event: KeyboardEvent) {
    event.preventDefault();
    if (event.key === "Escape") return setRecording(false);
    const key = keyName(event.key);
    if (!key) return;
    const mods = [
      event.ctrlKey && "ctrl",
      event.altKey && "alt",
      event.shiftKey && "shift",
      event.metaKey && "super",
    ].filter(Boolean);
    onChange([...mods, key].join("+"));
    setRecording(false);
  }

  return (
    <button
      type="button"
      className={recording ? "text shortcut recording" : "text shortcut"}
      onClick={() => setRecording(true)}
      onBlur={() => setRecording(false)}
      onKeyDown={recording ? onKeyDown : undefined}
    >
      {recording ? "Press keys…" : label(value)}
    </button>
  );
}

function keyName(key: string): string | null {
  if (key === " ") return "space";
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) return key.toLowerCase();
  if (/^[a-z0-9]$/i.test(key)) return key.toLowerCase();
  return null;
}

function label(shortcut: string): string {
  const mac: Record<string, string> = { ctrl: "⌃", alt: "⌥", shift: "⇧", super: "⌘" };
  const other: Record<string, string> = { ctrl: "Ctrl", alt: "Alt", shift: "Shift", super: "Super" };
  const parts = shortcut.split("+").map((part) => {
    const name = part.trim().toLowerCase();
    if (name === "space") return "Space";
    return (isMac ? mac : other)[name] ?? name.toUpperCase();
  });
  return parts.join(isMac ? "" : " + ");
}

/** The color picker only takes #rrggbb. */
function expand(color: string): string {
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(color);
  if (short) return `#${short[1]}${short[1]}${short[2]}${short[2]}${short[3]}${short[3]}`;
  return /^#[0-9a-f]{6}$/i.test(color) ? color : "#ff5a1f";
}
