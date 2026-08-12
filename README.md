# Agent Workbench (V0)

本地多 agent 开发工作台的 **V0 垂直切片**：两任务依赖（修函数 → 写测试），打通

1. **ACP** 结构化 UI（仅 OpenCode：`opencode acp`）
2. **确定性任务图** + 真依赖边（下游 worktree 必须拿到上游产物）
3. **L0 规则 / L1 mock 记忆分家** + ACE 式假提案 Inbox（人批准才进 L2）

> 文档可能过期；**以本仓库代码与测试为准**。

架构草图见 [docs/architecture.md](docs/architecture.md)。

---

## 仓库结构

| 路径 | 作用 |
|------|------|
| `crates/agent-core` | SQLite journal、worktree、scheduler、gate、merge、memory |
| `crates/agent-acp` | OpenCode ACP stdio JSON-RPC 客户端 + 事件规范化 |
| `crates/agent-daemon` | CLI / 给 Tauri 用的 daemon 门面 |
| `apps/desktop` | Tauri 2 + React Composer UI |
| `fixtures/demo-project` | 故意写坏的 `add` + 会失败的测试 |
| `fixtures/mock-agent` | CI 用 mock ACP 进程 |

---

## 环境要求

- Rust **1.85+**（推荐 `rustup default stable`；本机验证用过 1.97）
- Node 20+（前端）
- `git`、`python3`（mock agent）
- 可选：[`opencode`](https://opencode.ai/docs/acp/) 在 `PATH` 上（真 ACP）
- 桌面壳额外需要 Tauri 2 Linux 依赖（`webkit2gtk` / `libgtk-3-dev` 等）——见下文

---

## 快速跑（不依赖 OpenCode / Tauri）

```bash
# 1) Rust 核测试（journal / worktree 依赖边 / ACP normalize+mock）
cargo test --workspace

# 2) 准备演示 fixture 的 git 仓库
./scripts/init-demo-fixture.sh

# 3) 双任务 mock 调度（A 修函数 → gate → merge → B 看见上游产物）
cargo run -p agent-daemon -- \
  --data-dir .agent-workbench \
  --repo fixtures/demo-project \
  run-mock --yaml fixtures/demo-project/graph.yaml

# 4) ACP mock 契约冒烟
cargo run -p agent-daemon -- acp-smoke

# 5) 记忆演示：L0/L1/提案
cargo run -p agent-daemon -- --data-dir .agent-workbench seed-memory
# 按输出里的 id：
# cargo run -p agent-daemon -- --data-dir .agent-workbench invalidate-l1 <l1_id>
# cargo run -p agent-daemon -- --data-dir .agent-workbench approve-proposal <proposal_id>

# 6) 前端（fixture 回放，无需 Tauri）
cd apps/desktop && npm install && npm run build && npm run preview
```

人贴 YAML 也可直接改 `fixtures/demo-project/graph.yaml` 或 CLI 传入任意文件。

---

## UI 硬约束（已实现于 React）

- Thought：仅最新一段默认展开；用户点过后 `user-overridden`，系统不再自动改
- 用户输入：`>` 引用行，不是聊天气泡
- `PermissionBar` 在输入框**上方**；模型/思考/上下文在输入框内右下角
- **未广告**的 `model` / `thought_level`：DOM 不挂（空菜单不画）
- 热切失败或未广告 → 文案强制「新会话+摘要」，禁止假切
- 不做第四 IDE（仅外链打开编辑器按钮）
- 工具卡分区渲染 **Diff** / **Terminal**

无 OpenCode 时，UI / daemon 用 **fixture 事件回放** 渲染假会话 transcript。

---

## 桌面打包（Tauri 2）

本 CI/云环境 **缺少** `gdk-3.0` / webkit2gtk pkg-config，因此 **未在此环境验证** `tauri build`。

本机（Debian/Ubuntu 示例）：

```bash
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf pkg-config
cd apps/desktop
npm install
npm run tauri dev
# 或
cargo build --manifest-path src-tauri/Cargo.toml
```

`apps/desktop/src-tauri` **有意不加入** Cargo workspace members，避免无系统库时 `cargo test --workspace` 红灯。

---

## 演示剧本对照

| 步骤 | 状态 |
|------|------|
| fixture 项目（坏 `add` + 失败测试） | 有 `fixtures/demo-project` |
| 人贴 2 节点 YAML，B depends_on A + artifacts | `graph.yaml` / CLI |
| A/B 真 worktree；A gate 红 → 不 merge、B 不启动 | 单测 `gate_red_blocks_downstream` |
| A 绿 → merge/bootstrap → B 见上游文件；缺产物硬失败 | 单测 + `run-mock` 演示 |
| L0 只读规则逐字注入 + 审计事件 | 单测 `l0_injected_verbatim_auditable` |
| L1 invalidate 后行仍在 | 单测 + CLI |
| 假提案 Inbox，批准后进 `l2_bullets` | 单测 + CLI |
| 真 OpenCode 端到端 | **未验证**（本机无 `opencode`） |
| Tauri 桌面窗口 | **未验证**（缺 webkit/gtk） |

---

## 已知未验证 / 缺口

- 真实 `opencode acp` 进程的 initialize/session/configOptions 探测（仅 mock 测过）
- Tauri 窗口实测、系统 tray、原生菜单
- Merge 在复杂 git 冲突下的策略（V0 有 fallback copy，已 journal 标记 `mode`）
- 进程池热复用的长期泄漏/回收
- 权限「记住」跨重启的 UI 完整流（DB 表与 `op_type` 键已有，端到端 UI 未接真 ACP）

---

## 许可证

MIT
