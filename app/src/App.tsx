import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { Search } from "lucide-react";
import { type KeyboardEvent, type MouseEvent, useEffect, useLayoutEffect, useRef, useState } from "react";
import { age, size } from "./format";
import { KindIcon } from "./kinds";
import "./styles.css";

type Hit = {
  path: string;
  name: string;
  folder: string;
  kind: string;
  size: number | null;
  modified: number;
};

type Results = { hits: Hit[]; error: string | null };

const WIDTH = 720;
const BORDER = 2;
const BAR = 62;
const ROW = 52;
const LIST_PADDING = 12;
const FOOTER = 32;
const MESSAGE = 44;
const MAX_ROWS = 8;

const isMac = navigator.userAgent.includes("Mac");
const appWindow = getCurrentWindow();

export default function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Results | null>(null);
  const [selected, setSelected] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    const text = query.trim();
    if (!text) {
      setResults(null);
      return;
    }
    let current = true;
    invoke<Results>("search", { query: text })
      .then((reply) => {
        if (current) {
          setResults(reply);
          setSelected(0);
        }
      })
      .catch((err) => {
        if (current) setResults({ hits: [], error: String(err) });
      });
    return () => {
      current = false;
    };
  }, [query]);

  useEffect(() => {
    const unlisten = listen("sonar://shown", () => {
      input.current?.focus();
      input.current?.select();
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, []);

  const hits = results?.hits ?? [];
  const message = results?.error ?? (results && hits.length === 0 ? "No matches" : null);
  const rows = Math.min(hits.length, MAX_ROWS);
  const height =
    BORDER + BAR + (message ? MESSAGE : 0) + (rows ? rows * ROW + LIST_PADDING + FOOTER : 0);

  useEffect(() => {
    appWindow.setSize(new LogicalSize(WIDTH, height));
  }, [height]);

  useLayoutEffect(() => {
    list.current?.children[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const open = (hit: Hit, reveal: boolean) =>
    invoke(reveal ? "reveal" : "open", { path: hit.path }).catch((err) =>
      setResults({ hits, error: String(err) }),
    );

  function onKeyDown(event: KeyboardEvent) {
    const move = (by: number) => {
      event.preventDefault();
      setSelected((current) => Math.max(0, Math.min(hits.length - 1, current + by)));
    };
    switch (event.key) {
      case "ArrowDown":
        return move(1);
      case "ArrowUp":
        return move(-1);
      case "PageDown":
        return move(MAX_ROWS);
      case "PageUp":
        return move(-MAX_ROWS);
      case "Enter": {
        const hit = hits[selected];
        if (hit) {
          event.preventDefault();
          open(hit, event.ctrlKey || event.metaKey);
        }
        return;
      }
      case "Escape":
        event.preventDefault();
        if (query) setQuery("");
        else appWindow.hide();
        return;
    }
  }

  return (
    <main className="panel">
      <label className={hits.length || message ? "bar divided" : "bar"}>
        <Search className="bar-icon" size={22} strokeWidth={2} />
        <input
          ref={input}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Search files"
          autoFocus
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
        />
      </label>

      {message && <p className={results?.error ? "message error" : "message"}>{message}</p>}

      {rows > 0 && (
        <>
          <ul ref={list} className="results" role="listbox">
            {hits.map((hit, i) => (
              <li
                key={hit.path}
                role="option"
                aria-selected={i === selected}
                className="hit"
                onMouseMove={() => setSelected(i)}
                onClick={(event: MouseEvent) => open(hit, event.ctrlKey || event.metaKey)}
              >
                <KindIcon kind={hit.kind} />
                <span className="hit-text">
                  <span className="hit-name">{hit.name}</span>
                  <span className="hit-folder">{hit.folder}</span>
                </span>
                <span className="hit-meta">
                  {age(hit.modified)}
                  {hit.size !== null && ` · ${size(hit.size)}`}
                </span>
              </li>
            ))}
          </ul>
          <footer className="footer">
            <span>
              {hits.length} {hits.length === 1 ? "result" : "results"}
            </span>
            <span className="keys">
              <kbd>↵</kbd> Open
              <kbd>{isMac ? "⌘ ↵" : "Ctrl ↵"}</kbd> Show in folder
              <kbd>Esc</kbd> Close
            </span>
          </footer>
        </>
      )}
    </main>
  );
}
