# Orca / Paseo / Codeg / AionUi 全面对比与打分

- 对比日期：2026-08-16
- 对比对象：四款「CLI 编码 Agent 之上的控制面 / 工作台」，不是 Cursor / Claude Code / Codex 本身
- 方法：以各项目官网、GitHub README / 架构文档、GitHub API 元数据为主；第三方评测只作交叉印证，不单独当事实
- 局限：本报告未在同一台机器上对四款做同题实操评测。分数是产品定位与公开能力对照，不是盲测榜

## 1. 先给结论

这四家不是同一类产品，硬比「谁最强」会比歪。

| 产品 | 一句话定位 | 更像什么 |
| --- | --- | --- |
| **Orca** | Agent Development Environment（ADE）：worktree 隔离的并行编码编排 IDE | 给「一堆 CLI Agent」用的桌面指挥舱 |
| **Paseo** | 自托管 daemon + 多端客户端，手机/桌面/Web/CLI 同权遥控本地 Agent | 远程编排网关 |
| **Codeg** | 统一编码工作区：会话聚合 + 主 Agent 委托子 Agent + 桌面/Server/Docker | 工程工作台 + 协作总线 |
| **AionUi** | 本地优先的 Cowork 桌面：内置 Agent + 办公助手 + Team Mode | Claude Cowork 的开源跨平台替代 |

按「编码 Agent 控制面」这个共同交集排序：

1. **Orca** — 并行 worktree、diff 批注、内嵌浏览器 Design Mode、GitHub/Linear、SSH 远程 worktree，ADE 完成度最高
2. **Paseo** — 原生 iOS/Android 与桌面同权、无遥测、E2E relay，跨端最强；一等公民 Agent 数量明显更少
3. **Codeg** — 会话聚合和跨 Agent 委托最完整，Server/Docker 自托管清楚；社区最小
4. **AionUi** — 办公/定时任务/内置助手最强，编码 ADE 不是主场

按场景选：

- 桌面并行编码、要比多家 Agent 谁写得好 → **Orca**
- 离开工位还要用手机盯 Agent、要零遥测 → **Paseo**
- 要一个可自托管的统一工作区，还要 Agent 互相委托 → **Codeg**
- 要 PPT/Excel/Word、定时任务、非开发者也能用 → **AionUi**

等权总分（见第 6 节）AionUi 略高，是因为「办公 Cowork」和「上手」两维把它拉上去了。若你的真实需求是编码编排，应看场景加权，不要看等权总分。

## 2. 事实卡（截至 2026-08-16，GitHub API）

| 项 | Orca | Paseo | Codeg | AionUi |
| --- | --- | --- | --- | --- |
| 仓库 | [stablyai/orca](https://github.com/stablyai/orca) | [getpaseo/paseo](https://github.com/getpaseo/paseo) | [xintaofei/codeg](https://github.com/xintaofei/codeg) | [iOfficeAI/AionUi](https://github.com/iOfficeAI/AionUi) |
| 官网 | [onorca.dev](https://onorca.dev) | [paseo.sh](https://paseo.sh) | [docs.codeg.app](https://docs.codeg.app) | [aionui.com](https://www.aionui.com) |
| Stars | 46,455 | 13,913 | 2,758 | 32,033 |
| Forks | 3,243 | 1,443 | 343 | 3,282 |
| `open_issues`（含 PR） | 3,974 | 853 | 160 | 815 |
| 许可 | MIT | AGPLv3（LICENSE 写明主体 AGPLv3） | Apache-2.0 | Apache-2.0 |
| 创建 | 2026-03-17 | 2025-10-13 | 2026-02-09 | 2025-08-07 |
| 最新发行 | v1.4.183（2026-08-15） | v0.4.0（2026-08-13） | v0.26.0（2026-08-16） | v2.1.56（2026-08-14） |
| 主语言 / 壳 | TypeScript | TypeScript；daemon Node，桌面 Electron，移动端 Expo | Rust + Tauri 2 + Next.js 16 | TypeScript + Electron；后端另有 AionCore |
| 软件本身收费 | 免费；用你已有的 Agent 订阅 / API | 免费；赞助可选 | 免费 | 免费 |
| 遥测 | 打包版默认走 PostHog Cloud（美国），可关 | 官方称无遥测、无强制登录 | 本地 SQLite；未见官方遥测页 | 官方称数据只落本地 SQLite，不经他们的服务器 |

`open_issues` 是 GitHub 仓库字段，issue 和 PR 算在一起。Orca 数字大，既反映用户量大、发版快，也反映未关闭工单堆积；不能单独当成「质量差」。

## 3. 各自在解决什么问题

### 3.1 Orca：ADE，不是又一个 IDE

官方自称 ADE（Agent Development Environment）。核心单元是 **git worktree**，不是文件，也不是单条 chat。

公开能力（[onorca.dev](https://onorca.dev)、[README](https://github.com/stablyai/orca)）：

- 一条 prompt 扇出到多个 Agent，每个 Agent 独立 worktree，互不覆盖工作区文件
- Ghostty 风格终端（WebGL、无限分屏、重启恢复 scrollback）
- 每个 worktree 一个 Chromium 窗口（Design Mode）：点 UI 元素把 HTML/CSS/截图丢给 Agent
- 应用内 GitHub / Linear：从 issue/PR 开 worktree，在应用内审、批注、开 PR
- SSH worktree：Agent 跑在远程机器，带自动重连和端口转发
- 行级 diff 批注，批量回传给 Agent
- `orca` CLI：`worktree create` / `snapshot` / `click` / `fill`，Agent 也能驱动 Orca
- 手机 companion：看状态、跟进、切账号；不是完整桌面 ADE 的同权移植
- 宣称支持 25+ 内置 CLI Agent，其余「能在终端跑就能在 Orca 里跑」
- YC 背景；官方说「几乎每天发版」

明确短板：

- 打包版默认采集匿名产品遥测（PostHog Cloud, US）。文档写明不传 prompt、文件、仓库名、路径；可用 Settings 或 `DO_NOT_TRACK=1` / `ORCA_TELEMETRY_DISABLED=1` 关闭。[来源](https://www.onorca.dev/docs/telemetry)
- worktree 隔离只保证「不改同一份工作区文件」。同端口、同本地数据库、worktree 外的文件仍可能互踩。[第三方说明](https://medium.com/@creativeaininja/orca-runs-ten-coding-agents-at-once-without-letting-them-overwrite-each-other-9287dd10f6cf)
- 公开 issue 里出现过「删一个 workspace 误杀同路径其它 session」（[#10252](https://github.com/stablyai/orca/issues/10252)）以及 worktree reconcile 空转吃 CPU 的报告
- 假设你已经会用 CLI Agent 和 git。单 Agent、单任务时收益有限

### 3.2 Paseo：daemon 才是产品

架构是 **本机 daemon + 多客户端**，不是「再做一个桌面 IDE」。daemon 管 Agent 生命周期，客户端经 WebSocket 连上来（直连或可选 E2E relay）。[架构文档](https://github.com/getpaseo/paseo/blob/HEAD/docs/architecture.md)

公开能力（[paseo.sh](https://paseo.sh)、README）：

- 客户端：iOS、Android、桌面（Electron）、Web、CLI；官方强调手机与桌面功能同权
- 远程：可选官方 E2E relay（他们声称读不了流量），或 Tailscale / 自建隧道 / 直接暴露端口
- 语音：默认本地转写；可选用 OpenAI speech
- worktree 可选，不是强制核心隐喻
- 应用内预览、inline review、commit、开 PR、merge
- CLI 与 UI 同权：`paseo run` / `ls` / `attach` / `send`
- 可选 Paseo Hub：给 daemon 接 GitHub / Slack / Discord 触发
- 官方隐私立场：无遥测、无 tracking、无强制登录
- 架构文档还提到 schedule（cron）、loop、agent 间 chat、MCP 工具目录

一等公民 Agent（以官网 FAQ + 架构图为准，不采用第三方「39 个 Agent」口径）：

- Claude Code、Codex、Cursor、Copilot（架构图）、OpenCode、Pi

明确短板：

- 版本仍是 v0.4.0，产品比 Orca / AionUi 更早熟阶段
- AGPLv3：自用没问题；改完当网络服务对外提供，需要遵守 copyleft
- 一等公民 Agent 远少于 Orca「任何 CLI」和 AionUi / Codeg 的长名单
- 办公文档、内置助手不是方向

### 3.3 Codeg：工作区总线，不是纯启动器

官方定义：把多个编码 Agent 收进同一个工作区，做会话聚合和多智能体协作；部署形态是桌面、独立 `codeg-server`、或 Docker。[README](https://github.com/xintaofei/codeg)

公开能力：

- **会话聚合**：从 Claude Code / Codex / OpenCode / Gemini CLI / OpenClaw / Cline / Hermes / CodeBuddy / Kimi Code / Pi / Grok Build / Cursor 的本地会话目录导入，统一可搜
- **跨 Agent 委托**：主 Agent 通过 `codeg-mcp` sidecar 暴露 `delegate_to_agent`，在同一任务里把子任务交给另一类 Agent，各自独立 session
- 自 0.22 起可注册任意 ACP 兼容 Agent（内置约 12 个）
- 内置 `git worktree` 并行开发
- 工程闭环：文件树、编辑器、live diff、git、commit、嵌入终端
- 办公：捆绑 `officecli`，`.docx` / `.xlsx` / `.pptx` 创建/校对/编辑 + 页内预览
- 科研技能包（假设、实验设计、统计、可视化等）
- Automations：把 composer 配置存成可 cron / 手动的无头任务
- 聊天通道：Telegram、飞书、iLink（微信）
- 原生 iOS（SwiftUI）/ Android（Compose）客户端，文档写的是 **当前测试版**
- 凭证进 OS keyring，不进明文配置

明确短板：

- Stars 约 2.8k，生态和踩坑样本明显小于另外三家
- 桌面 + Server + Docker + 通道 + 办公 + 科研，表面多，默认路径不如 AionUi「装完就能聊」干净
- 移动端仍是 test release
- 赞助区有不少国内 API 中转广告，产品本身开源，但官网气质偏「工具 + 渠道」

### 3.4 AionUi：Cowork 桌面，编码只是其中一条腿

官方对标的是 Claude Cowork，不是 Orca 这种 ADE。[aionui.com](https://www.aionui.com)、[README](https://github.com/iOfficeAI/AionUi)

公开能力：

- **内置 Agent 引擎**：不装任何 CLI 也能用；贴 API key 即可
- 自动检测本机已装的 Claude Code、Codex、Gemini CLI、OpenCode、OpenClaw、Goose、Copilot、Kimi CLI 等 20+ CLI
- 21 个内置助手：PPT / Morph PPT / Excel / Word / 财报模型 / 学术论文 / UI / 3D 游戏等
- **Team Mode**：Leader 拆任务，经 Team MCP 分给 Teammate，mailbox + 共享任务板；外部 Agent 走 ACP
- 定时任务（cron），宣传 24/7
- 远程：WebUI + Telegram / 飞书 / 钉钉 / 微信（官网也写 Slack / Discord / WhatsApp）
- 统一 MCP：配一次，同步到各 Agent
- 本地 SQLite；官方称不经他们的服务器
- 跨平台 Electron（macOS / Windows / Linux）
- 中文社区和贡献者密度明显高于另外三家

明确短板：

- 不是 worktree-first 的编码 ADE；Team Mode 默认共享同一文件夹，并行写代码时隔离弱于 Orca
- Electron 内存和长期运行成本高于 Tauri（Codeg）
- Team Mode 仍在补坑：MCP TCP 内存暴增、teammate stand-by 触发 300s LLM 超时、静默 Agent 导致 Leader 死等，都是 2026 年中仍在修的真实故障（[#2429](https://github.com/iOfficeAI/AionUi/pull/2429)、[#2426](https://github.com/iOfficeAI/AionUi/pull/2426)、[#2425](https://github.com/iOfficeAI/AionUi/pull/2425)）
- 编码能力完全取决于你接入的 Agent；AionUi 自己不提供更强的编程模型
- 多 Agent 并行会成倍烧 API；没有订阅天花板，账单靠用户自己管

## 4. 分维对照

### 4.1 多 Agent 模型（这是四家真正的差异）

| 模型 | 谁在用 | 含义 | 适合 |
| --- | --- | --- | --- |
| 扇出竞赛 | Orca（主） | 同一目标复制到 N 个隔离 worktree，人来选赢家 | 方案不确定、要对比 Claude vs Codex |
| 并行遥控 | Paseo（主） | 多个 Agent 同时跑，人在手机/桌面巡视、跟进 | 人离开工位、任务彼此独立 |
| 委托协作 | Codeg（主） | 主 Agent 把子任务派给异类 Agent | 一个任务里要混用各家强项 |
| Leader / Teammate | AionUi Team Mode | Leader 拆解 + 共享工作区 + mailbox | 办公交付、调研+写稿+做表并行 |

Orca 的「协作」更像比赛；Codeg / AionUi 的「协作」更像分工。Paseo 居中：能并行，团队语义弱于后两家。

### 4.2 端与部署

| | Orca | Paseo | Codeg | AionUi |
| --- | --- | --- | --- | --- |
| 桌面 | macOS / Windows / Linux，原生 ADE | Electron，包 daemon | Tauri 原生桌面 | Electron |
| 手机 | Companion（监控/跟进） | **原生 iOS + Android，官方称与桌面同权** | 原生 iOS/Android，**测试版** | 无原生 ADE；靠 IM + WebUI |
| Web | 不是主形态 | 有，可跟 daemon 走 | `codeg-server` 浏览器访问 | WebUI / `--remote` |
| 无头 / 服务器 | SSH worktree、VPS | daemon 可无头 + CLI | `codeg-server` + Docker 一等公民 | 可 headless WebUI |
| 远程通道 | 手机配对、SSH | E2E relay / Tailscale / 直连 | 浏览器 + IM 通道 | IM 通道 + WebUI |

跨端完成度：Paseo > Codeg（方向对，移动端未出测试）> Orca（companion 够用，不是同权）> AionUi（远程是「发指令」，不是「带着工程闭环出门」）。

### 4.3 工程闭环

| | Orca | Paseo | Codeg | AionUi |
| --- | --- | --- | --- | --- |
| 编辑器 | 有（宣传对标 VS Code 体验） | 弱于 ADE | 有（文件树 + 编辑 + diff） | 偏预览，不是 IDE |
| 终端 | Ghostty 级分屏 | 有，偏 session 流 | 嵌入终端 | 不是卖点 |
| worktree | **强制核心** | 可选 | 内置并行流 | 不是核心 |
| Diff / 批注 | 行级批注回传 Agent | inline review | live diff | 有 diff 预览 |
| GitHub/PR | 应用内浏览、开 PR | commit / PR / merge | git 账号与提交 | 弱 |
| 浏览器 / UI 反馈 | Design Mode（每 worktree 一个 Chromium） | 应用预览 | Project Boot 预览 | 文件/办公预览 |
| 办公文档 | 弱（Markdown/PDF/图预览） | 弱 | officecli + 页内预览 | **最强**（原生 PPT/Excel/Word 助手） |

### 4.4 隐私、许可、商业友好度

| | Orca | Paseo | Codeg | AionUi |
| --- | --- | --- | --- | --- |
| 代码是否经他们的云 | 否（Agent 仍走各自厂商 API） | 否 | 否 | 否 |
| 产品遥测 | 打包版默认开，可关 | 官方称无 | 未见对等遥测说明 | 官方称无上传 |
| 强制账号 | 无 | 无 | 无 | 无 |
| 许可 | MIT，二次开发最松 | AGPLv3，网络服务 copyleft | Apache-2.0 | Apache-2.0 |
| 企业二次封装 | 最友好 | 要评估 AGPL | 友好 | 友好 |

四家都是「编排层免费，模型/Agent 订阅另算」。AionUi 因为内置 Agent 直接打 API，比「只用 Claude Max 订阅」更容易把账单打穿。

## 5. 打分规则

每维 1–10。10 = 在该维接近当前开源控制面里的上限；5 = 能用但不是设计中心；3 以下 = 基本不覆盖。

维度：

1. 编码编排 / ADE 深度
2. 跨端与远程
3. 办公 / 非编码 Cowork
4. 隐私与自托管姿态
5. 开源许可对二次开发的友好度
6. 工程闭环（编辑、diff、git、PR、预览）
7. 多 Agent 协作语义（委托 / 团队，不只是并排开窗口）
8. 生态热度与迭代可见度
9. 上手（越高越容易）
10. 稳定性风险（越高越让人放心；会同时看发版成熟度和已知故障，不只看 star）

未做同题实操，故没有「代码质量 / 一次任务成功率」维。那一维四家都依赖底层 Claude Code / Codex 等，比控制面意义不大。

## 6. 分维分数

| 维度 | Orca | Paseo | Codeg | AionUi |
| --- | --- | --- | --- | --- |
| 1 编码编排 / ADE | **9.5** | 7.5 | 7.5 | 5.5 |
| 2 跨端与远程 | 7.5 | **9.5** | 8.0 | 7.0 |
| 3 办公 Cowork | 3.5 | 3.0 | 7.5 | **9.5** |
| 4 隐私 / 自托管 | 6.5 | **9.5** | 8.5 | 8.0 |
| 5 许可友好度 | **9.5** | 6.5 | 9.0 | 9.0 |
| 6 工程闭环 | **9.0** | 7.5 | 8.0 | 6.5 |
| 7 协作语义 | 8.0 | 7.0 | **8.5** | **8.5** |
| 8 生态与迭代 | **9.5** | 7.5 | 5.5 | 8.5 |
| 9 上手 | 6.5 | 7.5 | 6.0 | **8.5** |
| 10 稳定性（放心程度） | 5.5 | 6.5 | 7.0 | 6.0 |
| **等权合计 /100** | **75.0** | **72.0** | **75.5** | **77.0** |

稳定性一维的读法：

- Orca 5.5：日更 + 近 4k 未关工单 + 已公开的 worktree/session 误杀类问题。功能最满，操作面也最容易踩边
- Paseo 6.5：架构干净，但仍是 v0.4.0
- Codeg 7.0：工单少、表面收敛，但用户少，未知未知更多；这是「没被打过」而不是「已被打穿还稳」
- AionUi 6.0：Team Mode 近期仍在修超时和内存问题；Electron 长任务更吃资源

## 7. 场景加权（这个比等权有用）

分数改为 10 分制。

### 7.1 重度编码 ADE

权重：编排 25% + 工程闭环 20% + 协作 15% + 跨端 10% + 生态 10% + 稳定 10% + 许可 5% + 隐私 5%

| | 加权 |
| --- | --- |
| **Orca** | **8.4** |
| Codeg | 7.7 |
| Paseo | 7.6 |
| AionUi | 7.0 |

### 7.2 跨端远程盯盘

权重：跨端 30% + 隐私 20% + 编码 15% + 工程 10% + 上手 10% + 稳定 10% + 协作 5%

| | 加权 |
| --- | --- |
| **Paseo** | **8.4** |
| Codeg | 7.8 |
| Orca | 7.5 |
| AionUi | 7.1 |

### 7.3 办公 Cowork / 非纯编码

权重：办公 30% + 上手 20% + 协作 15% + 跨端 15% + 隐私 10% + 生态 10%

| | 加权 |
| --- | --- |
| **AionUi** | **8.5** |
| Codeg | 7.3 |
| Paseo | 6.6 |
| Orca | 6.3 |

### 7.4 一张图记住

```
                 编码 ADE
                    ▲
                    │
              Orca  ●
                    │
         Codeg ●    │    ● Paseo
                    │
                    │         跨端远程
                    └──────────►
                   ●
                 AionUi
                    办公 Cowork
```

Codeg 在三角形内部：三边都不占顶点，但没有明显短板。适合「我只要一个能自托管的统一工作区，桌面和服务器都能跑」。

## 8. 不该信的说法

- 「Paseo 支持 39 个 Agent」：第三方指南有这个数字；[paseo.sh FAQ](https://paseo.sh) 和架构图只保证 Claude Code / Codex / Cursor / Copilot / OpenCode / Pi。本报告用官方口径。
- 「Orca 免费所以数据更安全」：编排层确实不经 Orca 云，但打包版默认有 PostHog 产品遥测；底层 Agent 仍把代码发给 Anthropic / OpenAI 等。
- 「AionUi 能替代 Claude Code」：不能。它是壳 + 内置通用 Agent + 办公技能。硬编码质量仍看你挂的那个 CLI。
- 「Stars 高 = 更好用」：Orca / AionUi 热，Paseo 专，Codeg 小。热度维已经单独打分，不应再渗进其它维。

## 9. 怎么选（可执行）

已经在用 Claude Code + Codex，桌面为主，要并行对比结果：装 **Orca**。第一件事：Settings → Privacy 关掉遥测，或设 `ORCA_TELEMETRY_DISABLED=1`。

已经在用上述 Agent，且经常离开工位、要原生手机：装 **Paseo**。先只开直连或 Tailscale，确认需要再开 relay。

要会话从各 CLI 里捞回来、要 Claude 调 Codex 当子 Agent、还要 Docker/服务器：装 **Codeg**。移动端先当预览，不要当生产。

要给自己或非工程同事做 PPT/表/文档、要 cron、要中文 IM 遥控：装 **AionUi**。先给内置 Agent 设便宜默认模型，再开 Team Mode。

可以叠：Paseo 或 Orca 管编码 Agent，AionUi 管办公。Codeg 和 AionUi 办公能力重叠，一般不同时当主工作台。

## 10. 来源

### 一手

- Orca 官网：https://onorca.dev
- Orca 仓库：https://github.com/stablyai/orca （GitHub API：46,455 stars，MIT，v1.4.183）
- Orca 遥测：https://www.onorca.dev/docs/telemetry
- Paseo 官网：https://paseo.sh
- Paseo 仓库：https://github.com/getpaseo/paseo （13,913 stars，LICENSE 主体 AGPLv3，v0.4.0）
- Paseo 架构：https://github.com/getpaseo/paseo/blob/HEAD/docs/architecture.md
- Codeg 仓库：https://github.com/xintaofei/codeg （2,758 stars，Apache-2.0，v0.26.0）
- Codeg 文档：https://docs.codeg.app
- AionUi 官网：https://www.aionui.com
- AionUi 仓库：https://github.com/iOfficeAI/AionUi （32,033 stars，Apache-2.0，v2.1.56）

### 交叉印证（不当主证据）

- https://codepick.dev/en/guides/paseo-remote-agent-orchestrator/
- https://www.volanea.com/blog/orca-ai-coding-agents
- https://vibecodinghub.org/blog/orca-review
- https://www.drpang.ai/aionui-multi-agent-desktop-workbench/
- https://github.com/stablyai/orca/issues/10252
- https://github.com/iOfficeAI/AionUi/pull/2425
- https://github.com/iOfficeAI/AionUi/pull/2426
- https://github.com/iOfficeAI/AionUi/pull/2429

## 11. 缺口

- 未做四家同题实操（同一仓库、同一 prompt、同一组 Agent）
- 未测内存、CPU、长时间并行的量化数据
- 未审各家权限模型的威胁模型细节（SSH、relay、IM bot token）
- GitHub `open_issues` 未拆 issue / PR
- Paseo「39 Agent」与官方名单冲突，已标，未再穷尽源码里的 provider 目录

若要补实操分，下一步应固定：同一 repo、同一三连 prompt（修 bug / 加功能 / 写文档）、同一对 Claude Code + Codex，记录隔离、冲突、远程跟进、diff 回传四件事。
