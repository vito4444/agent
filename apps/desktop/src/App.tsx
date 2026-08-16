import { useEffect, useMemo, useState } from "react";
import {
  approveProposal,
  invalidateL1,
  loadBootstrap,
  loadMenus,
  memorySnapshot,
  runMockGraph,
} from "./api";
import { Composer } from "./components/Composer";
import { Transcript } from "./components/Transcript";
import type { AdvertisedMenus, NormalizedEvent, StartupBanner } from "./types";

export default function App() {
  const [banners, setBanners] = useState<StartupBanner[]>([]);
  const [events, setEvents] = useState<NormalizedEvent[]>([]);
  const [yaml, setYaml] = useState("");
  const [input, setInput] = useState("");
  const [menus, setMenus] = useState<AdvertisedMenus>({ model_config: [] });
  const [inplace, setInplace] = useState(false);
  const [runResult, setRunResult] = useState<string>("");
  const [mem, setMem] = useState<Awaited<ReturnType<typeof memorySnapshot>> | null>(
    null,
  );
  const [modelHint, setModelHint] = useState<string | null>(
    "模型菜单仅在 session 广告 model/thought_level 后显示；热切失败则强制「新会话+摘要」，禁止假切。",
  );

  const permission = useMemo(() => {
    const p = [...events]
      .reverse()
      .find((e) => e.kind === "permission_request");
    if (!p || p.kind !== "permission_request") return null;
    return {
      op_type: p.op_type || "unknown",
      title: p.tool_call_id || "tool",
    };
  }, [events]);

  useEffect(() => {
    (async () => {
      const boot = await loadBootstrap();
      setBanners(boot.banners);
      setEvents(boot.transcript);
      setYaml(boot.demo_yaml);
      setMenus(await loadMenus());
      setMem(await memorySnapshot());
      if (!boot.opencode_available) {
        setModelHint(
          "本机未检测到 OpenCode：空菜单不画；换模型请「新会话+摘要」。",
        );
      }
    })();
  }, []);

  async function refreshMemory() {
    setMem(await memorySnapshot());
  }

  return (
    <div className="app">
      <aside className="panel side">
        <div className="brand">AGENT WORKBENCH</div>
        <div className="muted">V0 · OpenCode ACP · SQLite journal</div>
        <div style={{ height: "0.75rem" }} />
        {banners.map((b, i) => (
          <div key={i} className={`banner ${b.level === "error" ? "error" : ""}`}>
            {b.message}
          </div>
        ))}
        <h3>Task Graph YAML</h3>
        <textarea className="yaml-box" value={yaml} onChange={(e) => setYaml(e.target.value)} />
        <div className="stack" style={{ marginTop: "0.75rem" }}>
          <label className="muted">
            <input
              type="checkbox"
              checked={inplace}
              onChange={(e) => setInplace(e.target.checked)}
            />{" "}
            ask/单文件原地（将标 ⚠）
          </label>
          <button
            type="button"
            className="primary"
            onClick={async () => {
              try {
                const r = await runMockGraph(yaml);
                setRunResult(JSON.stringify(r, null, 2));
              } catch (e) {
                setRunResult(
                  `未在 Tauri 内，或调度失败：${String(e)}\n可用 CLI: cargo run -p agent-daemon -- run-mock`,
                );
              }
            }}
          >
            运行双任务（mock executor）
          </button>
          {runResult ? <pre className="muted">{runResult}</pre> : null}
        </div>
      </aside>

      <main className="main">
        <Transcript events={events} />
        <Composer
          value={input}
          onChange={setInput}
          onSend={() => {
            if (!input.trim()) return;
            setEvents((ev) => [
              ...ev,
              { kind: "message", role: "user", text: input.trim() },
              {
                kind: "message",
                role: "agent",
                text: "（fixture）已收到。真会话需 OpenCode ACP。",
              },
            ]);
            setInput("");
          }}
          menus={menus}
          permission={permission}
          onPermission={(allow) => {
            setEvents((ev) =>
              ev.filter((e) => e.kind !== "permission_request").concat([
                {
                  kind: "message",
                  role: "agent",
                  text: allow
                    ? "permission allowed (by op_type)"
                    : "permission denied",
                },
              ]),
            );
          }}
          inplaceWarning={inplace}
          modelSwitchHint={modelHint}
        />
      </main>

      <aside className="panel side">
        <h3>L0 Rules</h3>
        {mem?.l0.map((r) => (
          <div className="item" key={r.id}>
            {r.content}
          </div>
        ))}
        <h3>L1 Facts (mock)</h3>
        {mem?.l1.map((f) => (
          <div className={`item ${f.invalid_at ? "invalid" : ""}`} key={f.id}>
            <div>{f.content}</div>
            {!f.invalid_at ? (
              <button
                type="button"
                style={{ marginTop: "0.35rem" }}
                onClick={async () => {
                  try {
                    await invalidateL1(f.id);
                  } catch {
                    // fixture path: soft-invalidate locally
                    setMem((m) =>
                      m
                        ? {
                            ...m,
                            l1: m.l1.map((x) =>
                              x.id === f.id
                                ? { ...x, invalid_at: new Date().toISOString() }
                                : x,
                            ),
                          }
                        : m,
                    );
                    return;
                  }
                  await refreshMemory();
                }}
              >
                Invalidate
              </button>
            ) : (
              <div className="muted">invalid_at={f.invalid_at}</div>
            )}
          </div>
        ))}
        <h3>Proposal Inbox</h3>
        {mem?.proposals
          .filter((p) => p.status === "pending")
          .map((p) => (
            <div className="item" key={p.id}>
              <div>{p.content}</div>
              <button
                type="button"
                className="primary"
                style={{ marginTop: "0.35rem" }}
                onClick={async () => {
                  try {
                    await approveProposal(p.id);
                    await refreshMemory();
                  } catch {
                    setMem((m) =>
                      m
                        ? {
                            ...m,
                            proposals: m.proposals.map((x) =>
                              x.id === p.id ? { ...x, status: "approved" } : x,
                            ),
                            l2: [
                              ...m.l2,
                              { id: `l2-${p.id}`, content: p.content },
                            ],
                          }
                        : m,
                    );
                  }
                }}
              >
                Approve → L2
              </button>
            </div>
          ))}
        <h3>L2 Bullets</h3>
        {mem?.l2.length ? (
          mem.l2.map((b) => (
            <div className="item" key={b.id}>
              {b.content}
            </div>
          ))
        ) : (
          <div className="muted">（批准前为空）</div>
        )}
      </aside>
    </div>
  );
}
