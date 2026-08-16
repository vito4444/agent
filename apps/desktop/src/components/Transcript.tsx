import { useMemo, useState } from "react";
import type { NormalizedEvent } from "../types";

type Props = {
  events: NormalizedEvent[];
};

type ToolView = {
  id: string;
  title: string;
  status?: string;
  diff?: string;
  terminal?: string;
};

function DiffBlock({ diff }: { diff: string }) {
  return (
    <pre>
      {diff.split("\n").map((line, i) => {
        const cls = line.startsWith("+")
          ? "diff-add"
          : line.startsWith("-")
            ? "diff-del"
            : "";
        return (
          <div key={i} className={cls}>
            {line || " "}
          </div>
        );
      })}
    </pre>
  );
}

export function Transcript({ events }: Props) {
  // Thought: only the latest segment expands by default.
  // Once the user toggles any thought, system stops auto-managing (user-overridden).
  const [userOverridden, setUserOverridden] = useState(false);
  const [openMap, setOpenMap] = useState<Record<number, boolean>>({});

  const thoughtIndexes = useMemo(() => {
    const idxs: number[] = [];
    events.forEach((e, i) => {
      if (e.kind === "thought") idxs.push(i);
    });
    return idxs;
  }, [events]);
  const latestThought = thoughtIndexes[thoughtIndexes.length - 1];

  const tools = useMemo(() => {
    const map = new Map<string, ToolView>();
    for (const e of events) {
      if (e.kind === "tool_call") {
        map.set(e.tool_call_id, {
          id: e.tool_call_id,
          title: e.title,
          status: e.status,
        });
      } else if (e.kind === "tool_call_update") {
        const prev = map.get(e.tool_call_id) || {
          id: e.tool_call_id,
          title: e.tool_call_id,
        };
        map.set(e.tool_call_id, {
          ...prev,
          status: e.status ?? prev.status,
          diff: e.diff ?? prev.diff,
          terminal: e.terminal ?? prev.terminal,
        });
      }
    }
    return [...map.values()];
  }, [events]);

  return (
    <div className="transcript">
      {events.map((e, i) => {
        if (e.kind === "message") {
          return (
            <div key={i} className={`quote-line ${e.role}`}>
              {e.text}
            </div>
          );
        }
        if (e.kind === "thought") {
          const autoOpen = !userOverridden && i === latestThought;
          const open = userOverridden ? !!openMap[i] : autoOpen;
          return (
            <details
              key={i}
              className="thought"
              open={open}
              onToggle={(ev) => {
                const el = ev.currentTarget;
                setUserOverridden(true);
                setOpenMap((m) => ({ ...m, [i]: el.open }));
              }}
            >
              <summary>Thought</summary>
              <div>{e.text}</div>
            </details>
          );
        }
        return null;
      })}

      {tools.map((t) => (
        <div className="tool-card" key={t.id}>
          <header>
            <span>{t.title}</span>
            <span className="muted">{t.status || "running"}</span>
          </header>
          {t.diff ? (
            <div className="tool-section">
              <h4>Diff</h4>
              <DiffBlock diff={t.diff} />
            </div>
          ) : null}
          {t.terminal ? (
            <div className="tool-section">
              <h4>Terminal</h4>
              <pre>{t.terminal}</pre>
            </div>
          ) : null}
        </div>
      ))}
    </div>
  );
}
