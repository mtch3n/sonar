import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { Search, TriangleAlert } from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { applyLook } from "./look";
import { Results, rowId } from "./Results";
import type { Outcome, Row, Section, View } from "./types";
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

export default function App() {
  const [query, setQuery] = useState("");
  const [sections, setSections] = useState<Section[]>([]);
  const [selected, setSelected] = useState(0);
  const [view, setView] = useState<View | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [round, setRound] = useState(0);
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
    const fit = () => {
      const height = Math.ceil(element.getBoundingClientRect().height);
      appWindow.setSize(new LogicalSize(width, height));
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(element);
    return () => observer.disconnect();
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

  async function choose(row: Row, alt: boolean) {
    const label = alt ? row.alt : row.action;
    if (!label || busy) return;
    setBusy(PROGRESS[label] ?? null);
    try {
      const outcome = await invoke<Outcome>("activate", { id: row.id, alt });
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
          choose(chosen, event.ctrlKey || event.metaKey);
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

      {visible.length > 0 && (
        <Results
          sections={visible}
          selected={current}
          listRef={list}
          onHover={setSelected}
          onChoose={choose}
        />
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
            </span>
          )}
        </footer>
      )}
    </main>
  );
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
