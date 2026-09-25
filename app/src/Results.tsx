import type { MouseEvent, Ref } from "react";
import { Glyph } from "./icons";
import type { Row, Section } from "./types";

type Props = {
  sections: Section[];
  selected: number;
  listRef: Ref<HTMLDivElement>;
  onHover: (index: number) => void;
  onChoose: (row: Row, alt: boolean) => void;
};

export const rowId = (row: Row) => `row-${row.id}`;

export function Results({ sections, selected, listRef, onHover, onChoose }: Props) {
  // A lone file list needs no label; anything else says where results come from.
  const titled = sections.length > 1 || sections.some((section) => section.key !== "files");
  let index = 0;
  return (
    <div ref={listRef} id="results" className="results" role="listbox" aria-label="Results">
      {sections.map((section) => (
        <div key={section.key} className="section" role="group" aria-label={section.title}>
          {titled && <div className="section-title">{section.title}</div>}
          {section.rows.map((row) => {
            const i = index++;
            return (
              <div
                key={row.id}
                id={rowId(row)}
                role="option"
                aria-selected={i === selected}
                className="row"
                onMouseMove={() => onHover(i)}
                onClick={(event: MouseEvent) => onChoose(row, event.ctrlKey || event.metaKey)}
              >
                <Glyph icon={row.icon} image={row.image} />
                <span className="row-text">
                  <span className="row-title">{row.title}</span>
                  {row.subtitle && <span className="row-subtitle">{row.subtitle}</span>}
                </span>
                {row.meta && <span className="row-meta">{row.meta}</span>}
              </div>
            );
          })}
          {section.pending && (
            <div className="row skeleton" aria-hidden>
              <span className="glyph" />
              <span className="row-text">
                <span className="skeleton-line" />
                <span className="skeleton-line short" />
              </span>
            </div>
          )}
          {section.message && (
            <p className={section.warning ? "note warning" : "note"}>{section.message}</p>
          )}
        </div>
      ))}
    </div>
  );
}
