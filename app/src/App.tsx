import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { Search, TriangleAlert } from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { applyLook } from "./look";
import { Results, rowId } from "./Results";
import type { Choice, Outcome, Row, Section, View } from "./types";
import "./styles.css";

const isMac = navigator.userAgent.includes("Mac");
const appWindow = getCurrentWindow();

/** What the footer says while an action that goes to GitHub runs. */
const PROGRESS: Record<string, string> = {
  Install: "Installing…",
  Update: "Updating…",
  Remove: "Removing…",
  Add: "Adding…",
};

type Notice = { text: string; warning: boolean };

/** The actions of a row, as the Ctrl + K list shows them. */
type Menu = { row: Row; selected: number };

function actionsOf(row: Row): { label: string; choice: Choice }[] {
  return [
    { label: row.action, choice: "action" as Choice },
    ...(row.alt ? [{ label: row.alt, choice: "alt" as Choice }] : []),
    ...row.more.map((label, n) => ({ label, choice: { more: n } as Choice })),
  ];
}

export default function App() {
  const [query, setQuery] = useState("");
  const [sections, setSections] = useState<Section[]>([]);
  const [selected, setSelected] = useState(0);
  const [view, setView] = useState<View | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [round, setRound] = useState(0);
  const [menu, setMenu] = useState<Menu | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const panel = useRef<HTMLElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const searched = useRef("");
  // Only leading spaces go: the space after a keyword like "plugins " is what opens it.
  const text = query.trimStart();

  useEffect(() => {
    if (!text) {
      setSections([]);
      searched.current = "";
      return;
    }
    let current = true;
    let first = true;
    const channel = new Channel<Section[]>();
    channel.onmessage = (batch) => {
      if (!current) return;
      const fresh = first;
      first = false;
      setSections((shown) => arrange(fresh ? batch : merge(shown, batch)));
      if (fresh && searched.current !== text) setSelected(0);
      searched.current = text;
    };
    invoke("search", { query: text, onResults: channel }).catch((err) => {
      if (current) setSections([failure(String(err))]);
    });
    return () => {
      current = false;
    };
  }, [text, round]);

  const loadView = useCallback(() => {
    invoke<View>("view").then(setView, () => {});
  }, []);

  const fill = useCallback((value: string) => {
    setQuery(value);
    setNotice(null);
    requestAnimationFrame(() => {
      input.current?.focus();
      input.current?.setSelectionRange(value.length, value.length);
    });
  }, []);

  useEffect(loadView, [loadView]);

  useEffect(() => {
    const stops = [
      listen("sonar://shown", () => {
        input.current?.focus();
        input.current?.select();
        loadView();
        setRound((n) => n + 1);
      }),
      listen("sonar://view", loadView),
      listen<string>("sonar://fill", ({ payload }) => fill(payload)),
    ];
    return () => {
      for (const stop of stops) stop.then((unlisten) => unlisten());
    };
  }, [loadView, fill]);

  useEffect(() => {
    if (!view) return;
    document.documentElement.style.setProperty("--rows", String(view.rows));
    return applyLook(view.theme, view.accent);
  }, [view]);

  // The window is as tall as the panel's content, so it never shows empty space.
  const width = view?.width ?? 720;
  useLayoutEffect(() => {
    const element = panel.current;
    if (!element) return;
    // Results arrive in several batches per keystroke; resizing the window for each
    // one made the bar stutter, so resize at most once a frame, and only when the
    // height really changed.
    let last = 0;
    let frame = 0;
    const fit = () => {
      const height = Math.ceil(element.getBoundingClientRect().height);
      if (height === last) return;
      last = height;
      appWindow.setSize(new LogicalSize(width, height));
    };
    fit();
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(fit);
    });
    observer.observe(element);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [width]);

  const visible = sections.filter((s) => s.rows.length > 0 || s.message || s.pending);
  const rows = visible.flatMap((s) => s.rows);
  const current = Math.min(selected, Math.max(0, rows.length - 1));
  const chosen: Row | undefined = rows[current];
  const settled = sections.length > 0 && sections.every((s) => !s.pending);
  const nothing = text !== "" && settled && visible.length === 0;
  const notices = view?.notices ?? [];

  useLayoutEffect(() => {
    if (!chosen) return;
    const row = document.getElementById(rowId(chosen));
    const title = row?.previousElementSibling;
    if (title?.classList.contains("section-title")) title.scrollIntoView({ block: "nearest" });
    row?.scrollIntoView({ block: "nearest" });
  }, [chosen]);

  async function choose(row: Row, choice: Choice) {
    const label = choice === "action" ? row.action : choice === "alt" ? row.alt : row.more[choice.more];
    if (!label || busy) return;
    setMenu(null);
    setBusy(PROGRESS[label] ?? null);
    try {
      const outcome = await invoke<Outcome>("activate", { id: row.id, choice });
      if (outcome.then === "fill") {
        fill(outcome.text);
      } else if (outcome.then === "refresh") {
        setNotice({ text: outcome.notice, warning: false });
        setRound((n) => n + 1);
      }
    } catch (err) {
      setNotice({ text: String(err), warning: true });
    } finally {
      setBusy(null);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    const actionsKey = event.key === "k" && (event.ctrlKey || event.metaKey);
    if (menu) {
      const actions = actionsOf(menu.row);
      const moveTo = (to: number) => {
        event.preventDefault();
        setMenu({ ...menu, selected: Math.max(0, Math.min(actions.length - 1, to)) });
      };
      if (event.key === "ArrowDown") return moveTo(menu.selected + 1);
      if (event.key === "ArrowUp") return moveTo(menu.selected - 1);
      if (event.key === "Enter") {
        event.preventDefault();
        return void choose(menu.row, actions[menu.selected].choice);
      }
      if (event.key === "Escape" || actionsKey) {
        event.preventDefault();
        return setMenu(null);
      }
      return;
    }
    if (actionsKey && chosen) {
      event.preventDefault();
      return setMenu({ row: chosen, selected: 0 });
    }
    const move = (by: number) => {
      event.preventDefault();
      setSelected(Math.max(0, Math.min(rows.length - 1, current + by)));
    };
    const page = view?.rows ?? 8;
    switch (event.key) {
      case "ArrowDown":
        return move(1);
      case "ArrowUp":
        return move(-1);
      case "PageDown":
        return move(page);
      case "PageUp":
        return move(-page);
      case "Enter":
        if (chosen) {
          event.preventDefault();
          choose(chosen, event.ctrlKey || event.metaKey ? "alt" : "action");
        }
        return;
      case "Escape":
        event.preventDefault();
        if (query) {
          setQuery("");
          setNotice(null);
        } else {
          appWindow.hide();
        }
        return;
    }
  }

  const status = busy ?? (view?.indexing ? "Indexing…" : null);

  return (
    <main ref={panel} className="panel">
      <label className="bar">
        <Search className="bar-icon" size={20} strokeWidth={1.75} aria-hidden />
        <input
          ref={input}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setNotice(null);
            setMenu(null);
          }}
          onKeyDown={onKeyDown}
          placeholder="Search"
          aria-label="Search"
          role="combobox"
          aria-expanded={rows.length > 0}
          aria-controls="results"
          aria-autocomplete="list"
          aria-activedescendant={chosen ? rowId(chosen) : undefined}
          autoFocus
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
        />
      </label>

      {(notices.length > 0 || notice || nothing) && (
        <div className="notes" role="status">
          {notices.slice(0, 3).map((line) => (
            <p key={line} className="warning">
              <TriangleAlert size={14} strokeWidth={2} aria-hidden />
              {line}
            </p>
          ))}
          {notice && <p className={notice.warning ? "warning" : undefined}>{notice.text}</p>}
          {nothing && (
            <p>
              {view?.indexing
                ? `No matches for “${text.trimEnd()}” yet. Sonar is still indexing your home folder.`
                : `No matches for “${text.trimEnd()}”`}
            </p>
          )}
        </div>
      )}

      {menu ? (
        <Results
          sections={[actionsSection(menu.row)]}
          selected={menu.selected}
          listRef={list}
          onHover={(selected) => setMenu({ ...menu, selected })}
          onChoose={(row) => {
            const n = Number(row.id.slice(ACTION_ID.length));
            choose(menu.row, actionsOf(menu.row)[n].choice);
          }}
        />
      ) : (
        visible.length > 0 && (
          <Results
            sections={visible}
            selected={current}
            listRef={list}
            onHover={setSelected}
            onChoose={(row, alt) => choose(row, alt ? "alt" : "action")}
          />
        )
      )}

      {rows.length > 0 && (
        <footer className="footer">
          <span className="status">{status}</span>
          {chosen && (
            <span className="keys">
              <span>
                {chosen.action}
                <kbd>↵</kbd>
              </span>
              {chosen.alt && (
                <span>
                  {chosen.alt}
                  <kbd>{isMac ? "⌘ ↵" : "Ctrl ↵"}</kbd>
                </span>
              )}
              {chosen.more.length > 0 && (
                <span>
                  Actions
                  <kbd>{isMac ? "⌘ K" : "Ctrl K"}</kbd>
                </span>
              )}
            </span>
          )}
        </footer>
      )}
    </main>
  );
}

const ACTION_ID = "action-";

/** A row's actions as a list of rows of their own. */
function actionsSection(row: Row): Section {
  return {
    key: "actions",
    title: `Actions for ${row.title}`,
    rank: 0,
    rows: actionsOf(row).map(({ label }, n) => ({
      ...row,
      id: `${ACTION_ID}${n}`,
      title: label,
      subtitle: null,
      meta: null,
      more: [],
    })),
    message: null,
    warning: false,
    pending: false,
  };
}

function arrange(sections: Section[]): Section[] {
  return [...sections].sort((a, b) => a.rank - b.rank);
}

/** Late answers from plugins replace their own section and leave the rest in place. */
function merge(shown: Section[], batch: Section[]): Section[] {
  const keys = new Set(batch.map((section) => section.key));
  return [...shown.filter((section) => !keys.has(section.key)), ...batch];
}

function failure(message: string): Section {
  return { key: "error", title: "Error", rank: 0, rows: [], message, warning: true, pending: false };
}
