import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronRight, Minus, Puzzle, RefreshCw, TriangleAlert, X } from "lucide-react";
import { type KeyboardEvent, type ReactNode, useEffect, useState } from "react";
import { cn } from "@/lib/utils";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemSeparator,
  ItemTitle,
} from "@/components/ui/item";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { applyLook } from "./look";
import { hint, withValue } from "./pluginSettings";
import type { Editor, PluginInfo, PluginSettings, Setting, SettingValue, Tool, Settings as Values } from "./types";
import "./page.css";

const isMac = navigator.userAgent.includes("Mac");
const platform = isMac ? "mac" : navigator.userAgent.includes("Windows") ? "windows" : "linux";

const DEFAULT_ACCENT = "#ff5a1f";
/** The calculator's id, whose exchange rates Settings can update. */
const CALCULATOR = "calculator";

export default function Settings() {
  const [editor, setEditor] = useState<Editor | null>(null);
  const [draft, setDraft] = useState<Values | null>(null);
  const [saved, setSaved] = useState<Values | null>(null);
  const [status, setStatus] = useState<{ text: string; error: boolean } | null>(null);
  const [market, setMarket] = useState("");

  useEffect(() => {
    document.documentElement.dataset.platform = platform;
    invoke<Editor>("settings_get").then((loaded) => {
      setEditor(loaded);
      setDraft(loaded.settings);
      setSaved(loaded.settings);
    });
  }, []);

  useEffect(() => {
    if (saved && editor) return applyLook(saved.appearance.theme, accentColor(saved.appearance.accent, editor));
  }, [saved, editor]);

  if (!editor || !draft || !saved) return null;
  const systemAccent = editor.systemAccent ?? DEFAULT_ACCENT;

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
    <div className="flex h-full flex-col bg-background">
      <TitleBar />
      <main className="flex-1 overflow-y-auto px-8 pt-7 pb-10">
        <div className="mx-auto flex max-w-[620px] flex-col gap-7">
          {editor.problem && (
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertDescription className="select-text">
                settings.toml {editor.problem}. The form shows the last settings that worked. Saving fixes the file
                and keeps the old one as settings.toml.bak.
              </AlertDescription>
            </Alert>
          )}

          <Group title="Search bar">
            <Row label="Shortcut" hint="Click, then press the keys you want">
              <ShortcutField value={draft.shortcut} onChange={(shortcut) => change({ ...draft, shortcut })} />
            </Row>
            <Row label="Theme">
              <ToggleGroup
                variant="outline"
                size="sm"
                spacing={0}
                value={[draft.appearance.theme]}
                onValueChange={(picked: string[]) => {
                  const theme = picked[0] as Values["appearance"]["theme"] | undefined;
                  if (theme) appearance({ theme });
                }}
                aria-label="Theme"
              >
                {(["system", "light", "dark"] as const).map((theme) => (
                  <ToggleGroupItem key={theme} value={theme}>
                    {theme[0].toUpperCase() + theme.slice(1)}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </Row>
            <Row label="Accent color" hint="The selected result's icon and the text cursor">
              <AccentField
                value={draft.appearance.accent}
                system={systemAccent}
                onChange={(accent) => appearance({ accent })}
              />
            </Row>
            <Row label="Width" hint="480 to 1600">
              <NumberField
                label="Width"
                value={draft.appearance.width}
                unit="px"
                onChange={(width) => appearance({ width })}
              />
            </Row>
            <Row label="Visible results" hint="How many show before the list scrolls, 3 to 20">
              <NumberField label="Visible results" value={draft.appearance.rows} onChange={(rows) => appearance({ rows })} />
            </Row>
          </Group>

          <Group title="Search">
            <Row label="Results to find" hint="When the search has no limit: filter">
              <NumberField
                label="Results to find"
                value={draft.search.limit}
                onChange={(limit) => change({ ...draft, search: { limit } })}
              />
            </Row>
            <Row label="Rescan everything every" hint="Changes show up within seconds; this catches any that slip by">
              <NumberField
                label="Rescan every"
                value={draft.index.rescan_minutes}
                unit="min"
                onChange={(rescan_minutes) => change({ ...draft, index: { rescan_minutes } })}
              />
            </Row>
            <Row label="Check for updates" hint="Look for new versions of Sonar on GitHub">
              <Switch
                aria-label="Check for updates"
                checked={draft.updates.check}
                onCheckedChange={(check) => change({ ...draft, updates: { check } })}
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
            {editor.plugins.map((info) => (
              <PluginRow
                key={info.id}
                info={info}
                values={own(info.id)}
                ratesPublished={editor.ratesPublished}
                onChange={(patch) => plugin(info.id, patch)}
                onValue={(setting, value) => setValue(info.id, setting, value)}
                onStatus={setStatus}
              />
            ))}
          </Group>

          <Group title="Marketplaces" note="GitHub repositories whose plugins you can install.">
            {draft.marketplaces.map((repo) => (
              <Row key={repo} label={repo}>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Remove ${repo}`}
                  onClick={() => change({ ...draft, marketplaces: draft.marketplaces.filter((m) => m !== repo) })}
                >
                  <X />
                </Button>
              </Row>
            ))}
            <Item size="sm">
              <ItemContent>
                <Input
                  value={market}
                  placeholder="owner/name or a GitHub link"
                  onChange={(e) => setMarket(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && addMarket()}
                  aria-label="Marketplace to add"
                  spellCheck={false}
                />
              </ItemContent>
              <ItemActions>
                <Button variant="outline" onClick={addMarket} disabled={!market.trim()}>
                  Add
                </Button>
              </ItemActions>
            </Item>
          </Group>
        </div>
      </main>

      <footer className="flex flex-none items-center gap-3 border-t px-6 py-3">
        <Button variant="link" className="px-0 text-muted-foreground" onClick={() => invoke("settings_open_file")}>
          Open settings.toml
        </Button>
        <span
          role="status"
          className={cn(
            "flex-1 truncate text-right text-sm select-text",
            status?.error ? "text-destructive" : "text-muted-foreground",
          )}
        >
          {status?.text}
        </span>
        <Button onClick={save} disabled={!dirty}>
          Save
        </Button>
      </footer>
    </div>
  );
}

function accentColor(accent: string, editor: Editor): string {
  return accent === "system" ? (editor.systemAccent ?? DEFAULT_ACCENT) : accent;
}

/** The window's own title bar: drags the window, and has its buttons where the system's aren't drawn. */
function TitleBar() {
  const appWindow = getCurrentWindow();
  return (
    <header
      className={cn("flex h-13 flex-none items-center gap-2 border-b pr-2.5", isMac ? "pl-21" : "pl-8")}
      data-tauri-drag-region
    >
      <h1 className="flex-1 text-[15px] font-semibold tracking-tight" data-tauri-drag-region>
        Settings
      </h1>
      {!isMac && (
        <div className="flex gap-0.5">
          <Button variant="ghost" size="icon-sm" aria-label="Minimize" onClick={() => appWindow.minimize()}>
            <Minus />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className="hover:bg-destructive/10 hover:text-destructive"
            aria-label="Close"
            onClick={() => appWindow.close()}
          >
            <X />
          </Button>
        </div>
      )}
    </header>
  );
}

function Group({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2" aria-label={title}>
      <h2 className="px-1 text-xs font-medium text-muted-foreground">{title}</h2>
      <ItemGroup className="rounded-xl border bg-card">{children}</ItemGroup>
      {note && <p className="px-1 text-xs text-muted-foreground">{note}</p>}
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <Item size="sm" className="flex-nowrap">
      <ItemContent className="min-w-0">
        <ItemTitle className="select-text">{label}</ItemTitle>
        {hint && <ItemDescription>{hint}</ItemDescription>}
      </ItemContent>
      <ItemActions>{children}</ItemActions>
    </Item>
  );
}

/** A plugin: its keyword and switch, and its own settings folded under it. */
function PluginRow({
  info,
  values,
  ratesPublished,
  onChange,
  onValue,
  onStatus,
}: {
  info: PluginInfo;
  values: PluginSettings;
  ratesPublished: number | null;
  onChange: (patch: { enabled?: boolean; keyword?: string | null }) => void;
  onValue: (setting: Setting, value: SettingValue) => void;
  onStatus: (status: { text: string; error: boolean }) => void;
}) {
  const [open, setOpen] = useState(false);
  const foldable = info.settings.length > 0 || info.id === CALCULATOR;
  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <Item size="sm" className="flex-nowrap">
        <ItemMedia variant="icon">
          {info.image ? <img src={info.image} alt="" className="size-5 rounded" /> : <Puzzle />}
        </ItemMedia>
        <ItemContent className="min-w-0">
          <ItemTitle>{info.name}</ItemTitle>
          {info.description && <ItemDescription>{info.description}</ItemDescription>}
          {info.problem && <ItemDescription className="text-destructive">{info.problem}</ItemDescription>}
        </ItemContent>
        <ItemActions>
          <Input
            className="w-28"
            value={values.keyword ?? ""}
            placeholder={info.keyword ?? "No keyword"}
            onChange={(e) => onChange({ keyword: e.target.value.trim() || null })}
            aria-label={`Keyword for ${info.name}`}
            spellCheck={false}
          />
          <Switch
            aria-label={`Use ${info.name}`}
            checked={values.enabled}
            onCheckedChange={(enabled) => onChange({ enabled })}
          />
          <CollapsibleTrigger
            render={
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`${info.name} settings`}
                className={cn(!foldable && "invisible")}
              />
            }
          >
            <ChevronRight className={cn("transition-transform", open && "rotate-90")} />
          </CollapsibleTrigger>
        </ItemActions>
      </Item>
      {foldable && (
        <CollapsibleContent className="bg-muted/40 pl-9">
          {info.settings.map((setting) => (
            <Row key={setting.key} label={setting.title} hint={hint(setting)}>
              <SettingField
                setting={setting}
                value={values[setting.key] ?? setting.default}
                onChange={(value) => onValue(setting, value)}
              />
            </Row>
          ))}
          {info.id === CALCULATOR && <RatesRow published={ratesPublished} onStatus={onStatus} />}
        </CollapsibleContent>
      )}
      <ItemSeparator className="last:hidden" />
    </Collapsible>
  );
}

/** When the exchange rates are from, and a button that downloads them now. */
function RatesRow({
  published,
  onStatus,
}: {
  published: number | null;
  onStatus: (status: { text: string; error: boolean }) => void;
}) {
  const [when, setWhen] = useState(published);
  const [updating, setUpdating] = useState(false);
  async function update() {
    setUpdating(true);
    try {
      setWhen(await invoke<number>("rates_update"));
      onStatus({ text: "Exchange rates updated", error: false });
    } catch (err) {
      onStatus({ text: String(err), error: true });
    } finally {
      setUpdating(false);
    }
  }
  const from = when
    ? `From ${new Date(when * 1000).toLocaleDateString(undefined, { dateStyle: "medium" })}`
    : "Not downloaded yet";
  return (
    <Row label="Exchange rates" hint={from}>
      <Button variant="outline" size="sm" onClick={update} disabled={updating}>
        {updating ? <Spinner data-icon="inline-start" /> : <RefreshCw data-icon="inline-start" />}
        Update now
      </Button>
    </Row>
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
        <Input
          className="w-52"
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
      return <Switch aria-label={setting.title} checked={value === true} onCheckedChange={onChange} />;
    case "choice":
      return (
        <Choice
          label={setting.title}
          value={String(value)}
          items={setting.options.map((option) => ({ value: option.value, label: option.title }))}
          onChange={onChange}
        />
      );
  }
}

function Choice({
  label,
  value,
  items,
  onChange,
}: {
  label: string;
  value: string;
  items: { value: string; label: string }[];
  onChange: (value: string) => void;
}) {
  return (
    <Select items={items} value={value} onValueChange={(picked) => picked !== null && onChange(String(picked))}>
      <SelectTrigger className="w-52" aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {items.map((item) => (
            <SelectItem key={item.value} value={item.value}>
              {item.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
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
  const items = [
    { value: NONE, label: none },
    ...tools.map((tool) => ({ value: tool.command, label: tool.name })),
    { value: OTHER, label: "Other command…" },
  ];
  return (
    <span className="flex flex-col items-end gap-1.5">
      <Choice
        label={label}
        value={other ? OTHER : value || NONE}
        items={items}
        onChange={(picked) => {
          setOther(picked === OTHER);
          if (picked !== OTHER) onChange(picked === NONE ? "" : picked);
        }}
      />
      {other && (
        <Input
          className="w-52"
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

/** The select values for "Other command…" and for no command, which the select
 * would show as a placeholder if it were empty; no real command starts with a NUL. */
const OTHER = "\u0000other";
const NONE = "\u0000none";

/** A number with its unit inside the field, so every control lines up on the right. */
function NumberField({
  value,
  unit,
  label,
  onChange,
}: {
  value: number;
  unit?: string;
  label: string;
  onChange: (n: number) => void;
}) {
  return (
    <InputGroup className="w-28">
      <InputGroupInput
        type="number"
        value={value}
        aria-label={label}
        className="tabular-nums [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
        onChange={(e) => onChange(Number(e.target.value))}
      />
      {unit && <InputGroupAddon align="inline-end">{unit}</InputGroupAddon>}
    </InputGroup>
  );
}

/** Sonar's own orange, then the accents GNOME offers. */
const ACCENTS: [string, string][] = [
  ["Sonar orange", DEFAULT_ACCENT],
  ["Blue", "#3584e4"],
  ["Teal", "#2190a4"],
  ["Green", "#3a944a"],
  ["Yellow", "#c88800"],
  ["Red", "#e62d42"],
  ["Pink", "#d56199"],
  ["Purple", "#9141ac"],
  ["Slate", "#6f8396"],
];

/** Swatches like GNOME's, drawn in the page: the system's color picker is a GTK
 * dialog on Linux that looks nothing like Sonar. */
function AccentField({
  value,
  system,
  onChange,
}: {
  value: string;
  system: string;
  onChange: (accent: string) => void;
}) {
  const preset = value === "system" || ACCENTS.some(([, color]) => color === value.toLowerCase());
  const [custom, setCustom] = useState(!preset);
  const picked = (color: string) => !custom && value.toLowerCase() === color;
  const swatch = "size-5.5 rounded-full ring-offset-2 ring-offset-card outline-none focus-visible:ring-2 focus-visible:ring-ring";
  const ring = "ring-2 ring-foreground";
  return (
    <span className="flex flex-col items-end gap-2">
      <span className="flex flex-wrap items-center justify-end gap-1.5" role="radiogroup" aria-label="Accent color">
        <Button
          variant="outline"
          size="xs"
          role="radio"
          aria-checked={picked("system")}
          className={cn("rounded-full ring-offset-2 ring-offset-card", picked("system") && ring)}
          onClick={() => {
            setCustom(false);
            onChange("system");
          }}
        >
          <span className="size-3 rounded-full" style={{ background: system }} />
          System
        </Button>
        {ACCENTS.map(([name, color]) => (
          <button
            key={color}
            type="button"
            role="radio"
            aria-checked={picked(color)}
            aria-label={name}
            title={name}
            className={cn(swatch, picked(color) && ring)}
            style={{ background: color }}
            onClick={() => {
              setCustom(false);
              onChange(color);
            }}
          />
        ))}
        <button
          type="button"
          role="radio"
          aria-checked={custom}
          aria-label="Custom color"
          title="Custom color"
          className={cn(
            swatch,
            "bg-[conic-gradient(#e62d42,#c88800,#3a944a,#2190a4,#3584e4,#9141ac,#d56199,#e62d42)]",
            custom && ring,
          )}
          onClick={() => {
            setCustom(true);
            if (value === "system") onChange(system);
          }}
        />
      </span>
      {custom && (
        <InputGroup className="w-28">
          <InputGroupAddon>
            <span className="size-3 rounded-full" style={{ background: value }} />
          </InputGroupAddon>
          <InputGroupInput
            value={value}
            onChange={(e) => onChange(e.target.value.trim())}
            aria-label="Custom accent color"
            placeholder={DEFAULT_ACCENT}
            spellCheck={false}
            autoFocus
          />
        </InputGroup>
      )}
    </span>
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
    <Button
      variant="outline"
      className={cn("min-w-36", recording && "border-ring ring-3 ring-ring/50")}
      onClick={() => setRecording(true)}
      onBlur={() => setRecording(false)}
      onKeyDown={recording ? onKeyDown : undefined}
    >
      {recording ? "Press keys…" : label(value)}
    </Button>
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
