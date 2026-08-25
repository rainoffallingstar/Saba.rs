# KataGo 功能完整审查报告

> 审查范围：当前工作树中的全部 KataGo 相关功能，包括安装与资产发现、模型下载、配置生成、GTP 传输、进程生命周期、连接与重连、对弈/genmove、普通分析、流式分析、分析结果持久化、全盘复盘、取消与节点导航、GPUI 集成、macOS 打包以及回归测试。
>
> 本报告最初由只读审查生成；随后已按修复计划实施。下方“修复状态”是当前工作树的权威摘要，历史问题章节保留用于追踪原因与测试依据。

## 1. 执行摘要

当前 KataGo 集成已经具备一条可工作的主路径：

```text
配置/发现引擎
→ 后台启动进程
→ GTP handshake
→ boardsize/clear_board/棋谱 replay
→ attached session
→ kata-analyze 流式输出
→ 解析候选着、PV、胜率、目数和 ownership
→ UI 投影与 SGF 属性写回
```

最近的前台冻结问题已经得到实质性缓解：握手、棋谱 replay、`kata-set-param`、流启动、50ms stream read、stop/drain 和进程清理不再直接运行在 GPUI 事件处理器中；分析 run 也有 generation、停止、dispose 和节点绑定。

## 修复状态（当前工作树）

本轮已完成以下可靠性修复：

1. 超时后的 GTP transport 会被 poison；bounded 与 streaming 写入都拒绝复用迟到响应可能污染的通道。
2. supervisor stop/drop 有截止时间，不再无条件等待继承 stdout/stderr 的后代进程关闭 reader thread。
3. session 使用显式普通/搜索/停止 timeout policy；`genmove` 与 bounded analyze 使用 120 秒搜索期限。
4. fresh `AnalysisStream` 的 replay 与 `kata-set-param` 逐条读取、校验完整 GTP response 后才进入流模式。
5. bounded/streaming analysis 使用 run ticket、node binding 与 disposal gate；用户 detach 后迟到 lease 不会重新连接角色。
6. engine generation、connect task 与 command task 均按角色隔离；连接等待使用带 role/generation/node 的 `PendingAnalysisRequest`。
7. 流式批次只更新 UI；只有显式 `AnalysisRunOutcome::Completed` 且 SGF 属性写入成功才持久化并推进全盘复盘。
8. 官方 `kata-analyze` 达到配置 `maxVisits` 后主动 stop/drain；代理 final 记录也可完成；EOF/准备失败明确失败，不显示完成 toast。
9. 全盘复盘计划包含 root baseline，并在完成、失败或取消后恢复原始选择。
10. setup 不再宣称可下载不存在的 macOS KataGo archive；模型选择确定化，Sabaki 管理的配置带 schema 版本且不覆盖用户配置。

当前发布前剩余验证重点是人工 GUI 保存/重开、安装包内真实路径和长局全盘复盘体验；协议、host、GPUI 与真实 KataGo 自动回归均已通过。

---

## 2. 审查对象与模块地图

### 2.1 安装、发现与配置

- `crates/sabaki-host/src/katago_setup.rs`
  - `HardwareBackend`
  - `KataGoModelTier`
  - 官方模型 URL
  - 硬件后端和二进制 archive 名称
  - `generate_optimized_gtp_config`
  - 模型下载 adapter 与原子替换
  - `ensure_katago_environment`
  - `find_katago_executable`
  - stale model path 修复
- `apps/sabaki-gpui/src/file_workflow.rs`
  - 内置 KataGo setup plugin 安装
- `examples/plugins/katago-setup-hub/`
  - declarative setup/download commands
- `scripts/bundle-macos.sh`
  - macOS `.app`/`.dmg` 构建

### 2.2 GTP 与进程

- `crates/domain-core/src/gtp.rs`
  - `GtpCommand`、`GtpResponse`
  - GTP response parser
  - `GtpProcessSupervisor`
  - stdout/stderr reader thread
  - bounded command、streaming command、timeout、stop/drop
- `crates/sabaki-host/src/engine_session.rs`
  - handshake、capability probe、board setup、command forwarding、streaming API
- `crates/sabaki-host/src/engine_controller.rs`
  - role → session map
  - attach/detach
  - command/analysis lease
  - replay、synchronize、session ownership
- `apps/sabaki-gpui/src/main.rs`
  - GPUI task、generation、角色连接和状态投影

### 2.3 分析与全盘复盘

- `crates/sabaki-host/src/analysis.rs`
  - Leela/KataGo GTP `info move` parser
  - JSON proxy parser
  - streaming process replay
- `crates/sabaki-host/src/analysis_controller.rs`
  - run generation、node/player binding、stop/dispose/replay flag
- `crates/sabaki-host/src/whole_game_review.rs`
  - active root→current lineage
- `apps/sabaki-gpui/src/engine_console.rs`
  - 分析命令、候选合并、胜率换算、PV 和 vertex 辅助逻辑
- `apps/sabaki-gpui/src/main.rs`
  - attached/fresh stream worker、批量 UI 更新、SGF 写回、全盘复盘推进
- `apps/sabaki-gpui/src/panels.rs`、`goban_view.rs`、`winrate_graph.rs`
  - UI 展示、ownership 热图和 winrate graph

### 2.4 测试与文档

- `crates/sabaki-host/tests/katago_regression.rs`
- `crates/sabaki-host/src/*` 中的 fixture/unit tests
- `docs/release-remediation-plan.md`
- `docs/handoff.md`

---

## 3. 已验证的正确点

### 3.1 GPUI 前台冻结路径已显著收敛，但仍需以实际 checkout 为准

在当前 checkout 中，`on_engine_connect` 把进程启动、握手、capability probe、board setup 和 replay 放入 `background_executor`，见 `apps/sabaki-gpui/src/main.rs:981-1046`。

当前版本的 attached streaming 路径也将 replay、`kata-set-param`、stream start、50ms 读取、stop 和 drain 放入 worker/background executor，见 `apps/sabaki-gpui/src/main.rs:1313-1546`；fresh `AnalysisStream` 路径见 `apps/sabaki-gpui/src/main.rs:1580-1737`。

审查过程中另一个基于较早行号/旧 checkout 的异步审查报告认为这些循环仍在 GPUI foreground。该结论与本文件所引用的当前源代码不一致，不能作为当前实现的直接事实；但它指出的架构目标仍应保留：分析 worker 应继续下沉为 `sabaki-host` 的 UI-independent seam，foreground 只处理 generation-gated events。后续重构必须保留确定性测试，防止重新把 blocking I/O、packed-line parsing 或 session cleanup 移回 GPUI 前台。

### 3.2 stdout/stderr 双管道都有持续读取

`GtpProcessSupervisor` 为 stdout 和 stderr 各自创建 reader thread，并保留 stderr tail，避免 KataGo stderr 写满导致子进程停止响应，见 `crates/domain-core/src/gtp.rs:150-200`。

### 3.3 当前分析节点有明确绑定

`AnalysisRunController` 保存 generation、node id 和 player；`player_for_node` 会阻止明显的跨节点结果写入，见 `crates/sabaki-host/src/analysis_controller.rs:64-186`。

### 3.4 全盘复盘使用 active lineage，并已存在真实 progression

当前 checkout 中，全盘复盘不是按 `moves.len()` 或颜色奇偶推断，而是通过节点父链取得 root→current 的真实活动分支；sibling variation 会被排除。`BatchReviewState` 保存 `original_node_id`、冻结的节点列表和 `next_index`；`analysis_finished` 在前一轮 worker 结束并归还/处置 session 后推进到下一 NodeId，最后恢复原节点。相关实现位于 `crates/sabaki-host/src/whole_game_review.rs` 和 `apps/sabaki-gpui/src/main.rs:1844-2037`。

审查过程中另一个基于旧源码行号的报告认为 whole-game review 只初始化进度、没有逐手 progression。该结论描述的是修改前实现，与当前 checkout 不一致，不能作为当前缺陷记录。该报告提出的风险仍有效并已纳入 Findings/测试缺口：ticket-bound NodeId、lease 归还后再推进、用户导航/编辑取消策略、空局、失败恢复、并发任务竞争和 fake-engine 端到端测试。

### 3.5 分析流解析已兼容真实 KataGo GTP

当前 parser 支持：

- `info move ... visits ... winrate ... scoreLead ... pv ...`
- KataGo 可能将多个候选和 `rootInfo` 打包在一行的情况
- JSON proxy 兼容格式
- ownership 数组

见 `crates/sabaki-host/src/analysis.rs:33-185`。

### 3.6 模型下载具备基本原子替换语义

模型先写到临时文件，检查非空后再替换 live 文件；失败或空文件不会覆盖既有模型，见 `crates/sabaki-host/src/katago_setup.rs:185-245`。

### 3.7 当前本机真实 KataGo 核心 smoke 已有证据

`crates/sabaki-host/tests/katago_regression.rs` 覆盖真实引擎的：

- handshake
- boardsize/clear_board
- replay
- `kata-set-param`
- `kata-analyze`
- 流式 `info move`
- stop/drain
- 后续重新开始分析

一次审查运行中该集成测试 3 项全部通过；另一轮审查中完整 session 的 `genmove` 超过 30 秒超时。这种差异本身构成 timeout policy 风险，不能只以单轮通过作为发布证明。

---

## 4. Findings：按优先级排序

## P0 — 阻塞自动安装或可能导致协议/数据错误

### P0-1：官方 KataGo 二进制 archive URL 已失效

**位置：** `crates/sabaki-host/src/katago_setup.rs:69-83`

`HardwareBackend::download_archive_name()` 返回：

```text
katago-v1.17.1-macos-arm64-metal.zip
katago-v1.17.1-macos-x86_64-opencl.zip
katago-v1.17.1-windows-x64-cuda12.zip
katago-v1.17.1-linux-x64-opencl.zip
katago-v1.17.1-linux-x64-eigen.zip
```

通过 GitHub release API 检查 `v1.17.1` asset 列表时，没有 macOS asset；实际 asset 命名包含具体 CUDA/cuDNN 版本，Linux/Windows 也不是上述通用名称。Apple Silicon URL 实测返回 HTTP 404。

**影响：** setup UI 宣称可自动配置/下载引擎，但实际无法从这些 URL 获取官方二进制。模型 URL 可以访问，不代表引擎安装链路可用。

**建议：**

1. 不要把不存在的 archive 名称硬编码为官方可下载资产。
2. 对每个支持平台使用已验证的发布资产映射，或明确改为“只配置本地/Homebrew 引擎”。
3. 下载前做 manifest/HEAD/HTTP 状态检查，并把平台不支持显示为明确状态。
4. 若采用第三方/Homebrew 资产，记录来源、版本和可执行文件布局。
5. 为每个平台加入 URL contract test，避免 release asset 改名后静默失效。

### P0-2：GTP bounded command 超时后会污染复用中的响应队列

**位置：** `crates/domain-core/src/gtp.rs:221-292`

`send_with_timeout` 将所有 stdout 行放入共享 `receiver`。如果命令在收到完整 header/body 前超时，函数直接返回 `CommandTimeout`，但：

- 已写入的 partial response 仍可能留在队列；
- 子进程可能稍后继续发送旧命令的 response；
- 后续复用同一个 transport 的命令会继续读取同一队列；
- 没有进入“transport poisoned / must restart”状态。

**影响：** 后续命令可能消费迟到的旧响应，导致错误命令对应、错误 response identifier、错误 replay 或错误 stop。这个问题比单次命令失败更危险，因为表现可能是偶发连接失效或分析流错乱。

**建议：**

1. 超时后将 transport 标记为 poisoned，不再允许复用。
2. 让 controller 丢弃并后台 stop 整个 session，而不是归还 lease。
3. 如果必须恢复，建立带命令 ID 的集中路由器，保存 pending response，并显式丢弃超时命令的尾部。
4. 添加 fake transport 测试：header 已到但 body 延迟、超时后迟到 response、下一命令不能消费旧 response。

---

## P1 — 高风险运行时缺陷

### P1-1：fallback `AnalysisStream` 不读取 replay/setup response

**位置：** `crates/sabaki-host/src/analysis.rs:211-229`；调用点 `apps/sabaki-gpui/src/main.rs:1584-1603`

`replay_position_stream` 逐行发送：

```text
boardsize
clear_board
play ...
```

随后 fresh stream worker 发送：

```text
kata-set-param maxVisits ...
kata-analyze ...
```

但 `AnalysisStream` 的这些 setup command 发送后没有读取并验证 GTP response。stdout 中的 setup header/terminator 会留给之后的分析读取循环。

**影响：**

- setup 命令失败可能被忽略；
- 后续读取可能首先读到 setup response，而不是 analysis info；
- 如果 setup response 与 stream output 交错，解析和 stop/drain 边界更难判断；
- 用户看到的是“分析无输出”或“引擎已启动但结果不更新”。

**建议：** 给 `AnalysisStream` 增加明确的 request/response API：每个 replay/setup command 都等待并校验 response；只有 `kata-analyze` 启动后才进入纯 stream 模式。若协议确实允许 setup response 异步到达，需建立 framing 状态机而不是简单跳过 header。

### P1-2：bounded `analyze` 完成路径缺少 stale generation 守卫

**位置：** `apps/sabaki-gpui/src/main.rs:1214-1262`

bounded `analyze` worker 取得 command lease 后在后台运行。完成回调直接：

- `analysis_task = None`
- 成功时 `return_command_lease`
- 调用 `set_analysis`
- `analysis_run.finish(&task_run)`

该路径没有像 streaming worker 那样在完成前检查：

- 当前 run 是否仍然有效；
- engine generation 是否已经改变；
- 是否已经执行 disconnect/cancel；
- 当前节点是否仍是 task 开始时的节点。

**影响：** 用户断开引擎、切换节点或开始新分析后，旧 bounded response 到达，可能归还已不应归还的 session、覆盖新分析、更新错误节点的分析 UI，甚至重新建立被断开的 role 状态。

**建议：** bounded worker 使用与 streaming 相同的 ticket gate：

1. 完成时先判断 `task_run.is_current()`/`should_dispose()`；
2. stale/disposed 时后台 stop 或 discard lease，不调用 `set_analysis`；
3. 只有 current run 才 return lease 和 apply result；
4. 增加“disconnect while bounded analyze is delayed”的确定性测试。

### P1-3：全局 `engine_generation` 使不同角色相互干扰

**位置：**

- `apps/sabaki-gpui/src/main.rs:843-874`
- `apps/sabaki-gpui/src/main.rs:1017-1019`
- `apps/sabaki-gpui/src/main.rs:1126-1133`
- `apps/sabaki-gpui/src/main.rs:2084-2114`

Analysis、White、Black、Console 共用同一个 `engine_generation`。任意角色连接/断开都会递增该值，所有异步 command/connect worker 都以此值判断 stale。

**影响：**

- 断开 Analysis 可能让 White 的 command worker 被误判 stale；
- 连接 White 可能让正在进行的 Analysis 连接或命令被误判 stale；
- 角色间的失败恢复不可预测，尤其在多角色同时使用时。

**建议：** 使用按角色分组的 generation：

```text
EngineRole -> RoleGeneration
```

或使用结构化 operation token：`{ role, generation, operation_kind }`。只有同一 role 的新连接/断开才能取消该 role 的旧工作。

### P1-4：超时策略对 KataGo 搜索命令过于粗糙

**位置：** `crates/domain-core/src/gtp.rs:110-115, 208-292`；测试 `crates/sabaki-host/tests/katago_regression.rs:332-336`

所有 bounded command 使用统一 `DEFAULT_COMMAND_TIMEOUT = 30s`。但 KataGo 的 `genmove` 或冷启动可能受模型加载、Metal/CUDA 初始化、CPU 负载和 visits 设置影响。

审查中出现两种结果：

- 一轮真实 KataGo 回归 3 项通过；
- 另一轮 `full_session_handshake_and_replay_with_real_katago` 的 `genmove` 超过硬编码 30 秒。

**影响：** 用户会把正常的慢搜索误判为引擎损坏；超时后还会触发 P0-2 的 transport desynchronization 风险。

**建议：**

1. 按命令类型区分 timeout：handshake、setup、console、genmove、analysis stop。
2. 将搜索 timeout 设为可配置，并提供“无限/取消优先”的 cooperative search 模式。
3. timeout 错误中携带 command、arguments、elapsed、stderr tail、model/config 信息。
4. timeout 后强制 poison/discard session，不能继续复用。
5. 用低 visits 和固定小棋盘建立快速 CI smoke，再单独运行硬件级长 smoke。

### P1-5：分析 stream setup 失败的错误语义不完整

**位置：** `apps/sabaki-gpui/src/main.rs:1584-1617`

fresh stream preparation 把 `start_in`、replay、`kata-set-param` 和 `kata-analyze` 串成一个 `and_then` 链，但 setup command response 未被完整读取/校验，失败的诊断只可能来自写入错误或进程退出。

**影响：** 配置参数不被 KataGo 接受时，UI 不能区分“进程没启动”“棋盘 replay 被拒绝”“参数无效”“分析命令未开始”。

**建议：** 返回结构化 preparation error：阶段、命令、GTP response、stderr tail，并在 UI 显示具体阶段。

### P1-6：stop/drop 的 join 没有有界退出策略

**位置：** `crates/domain-core/src/gtp.rs:344-365`

`stop()` 调用 `child.kill()` 后立即 join stdout/stderr reader thread，再 `child.wait()`。正常情况下管道会关闭，但如果子进程派生后代继承 stdout/stderr 文件描述符，或者 reader 没有及时结束，join 可能长时间甚至无限等待。

**影响：** 取消分析、断开引擎或应用退出时仍可能卡在 worker；如果未来某条 cleanup 路径回到前台，会重新引入冻结。

**建议：**

1. 明确 `Stopping` 状态和 deadline。
2. 先尝试协议级 `stop`/`quit`，再 kill。
3. 对 reader join 使用可观察的超时策略；超时记录诊断并避免阻塞 UI。
4. 必要时使用进程组/job object，确保子进程及其后代一起退出。
5. 增加继承 stdout/stderr 的 child fixture。

---

## P1 — 高风险运行时缺陷（续）

### P1-7：全盘复盘错误路径可能继续推进并报告完成

**位置：** `apps/sabaki-gpui/src/main.rs:1352-1367, 1483-1546, 1607-1617, 1705-1737, 1844-1893`

当前复盘推进集中在 `analysis_finished`。准备失败和 stdout EOF 的 worker 路径最终可能进入普通完成清理；如果没有明确的 `Completed / Cancelled / Failed` worker outcome，batch 状态机无法可靠区分“用户停止”“无输出失败”和“有效完成”。

**影响：** 某一手没有有效候选或没有成功写入 `SBKV`/`SBKS` 时，复盘仍可能跳到下一手，最后显示完成，但棋谱只保存了部分结果。

**建议：** worker 返回结构化 outcome；只有 `Completed` 且存在可持久化候选、属性写入成功时才推进。`Failed` 应停止 batch、保留失败手数和诊断，不显示完成 toast。

### P1-8：fallback 分析进程可能按节点反复冷启动

**位置：** `apps/sabaki-gpui/src/main.rs:1554-1605, 1729-1737, 1854-1877`

当 attached session 不支持 streaming 时，普通流程每次 `start_analysis` 都创建独立 `AnalysisStream`。全盘复盘的每个节点完成后会销毁该进程，再为下一节点重新启动。

**影响：** KataGo 模型重复加载、复盘时间急剧增加、搜索树丢失，并增加每轮启动失败的概率。

**建议：** 整个 batch 固定一种 source；复用一个长期 streaming worker/stream 会话，或明确使用 bounded analysis 全程处理，不要每一手重复启动大型模型。

### P1-9：全盘复盘缺少根局面基线

**位置：** `crates/sabaki-host/src/whole_game_review.rs` 的 active lineage 规划；`apps/sabaki-gpui/src/main.rs:1953-2010`

当前计划只包含带 `B/W` 的落子节点，从第一手落子后的局面开始分析，没有根局面/落子前基线。

**影响：** 第一手无法以落子前最佳着法作为比较基准；替代着法和损失可能归因到下一手节点，第一手评价与实际落子错位。

**建议：** 将 root position 作为 baseline 纳入计划，或为每一手建立 `(before_node, played_node)` pair，并明确结果绑定规则：第 N 手使用 N−1 局面的推荐着法和 N−1→N 的评估差。

### P1-10：全盘复盘缺少真实 3+ 节点端到端锁定测试

**位置：** `apps/sabaki-gpui/src/main.rs:1844-1893, 1953-2037`

现有 `BatchReviewState`/lineage 单测验证规划和索引，但尚未验证 fake engine 下的完整顺序：分析、stop/drain、归还 lease、按 NodeId 导航、下一轮、取消、恢复 original node 和 stale late result。

**影响：** 单节点成功不能证明长复盘不会跳手、重复启动、停在错误节点或假成功。

**建议：** 增加 scripted transport + headless GPUI 集成测试，覆盖 3+ 节点、失败/EOF、取消、文档编辑和最终恢复。

## P2 — 中风险正确性、性能和架构问题

### P2-1：leased role 的 detach 没有持久化“必须丢弃”意图

**位置：** `crates/sabaki-host/src/engine_controller.rs:116-135, 187-212, 254-280`

`detach(role)` 对 leased role 只返回 `true`，并不从 `leased_roles` 清除，也不记录 detach intent；实现依赖 UI worker 观察 `AnalysisRunController` 后自行 discard。

**影响：** controller 的通用调用者无法仅通过 `detach` 保证该角色最终不会被 worker return；若未来增加非 GPUI caller，旧 session 可能重新进入 controller。

**建议：** 把 lease token 变成显式 ownership object，并在 controller 中记录 `DetachRequested`/`Discarded` 状态。归还 lease 必须验证 token/generation。

### P2-2：`EngineController` 中 role state 由多个集合平行表示

**位置：** `crates/sabaki-host/src/engine_controller.rs:20-36`

` sessions`、`leased_roles`、`command_roles` 三个集合表达同一 role 的互斥状态。虽然当前方法有测试，但平行集合容易在异常/取消路径中出现组合状态不一致。

**建议：** 逐步收敛为 role state enum，例如：

```text
Detached
Ready(session)
CommandLeased
AnalysisLeased
Stopping
```

同时保留深模块 interface，不让 UI 操作内部状态。

### P2-3：模型下载没有 checksum/可信 digest 校验

**位置：** `crates/sabaki-host/src/katago_setup.rs:185-236`

当前只检查下载文件非空，然后原子替换。`docs/release-remediation-plan.md:217-220` 已明确记录官方固定 checksum 尚待补齐。

**影响：** 网络代理、损坏下载或被替换的 asset 可能被安装为模型；gzip 可打开不等于模型内容可信。

**建议：** 使用上游签名 manifest 或固定 SHA-256；下载前/后显示版本、文件大小和 digest；失败不覆盖旧模型。

### P2-4：环境发现会选择 models 目录中的第一个模型

**位置：** `crates/sabaki-host/src/katago_setup.rs:343-369`

没有 custom path 时，`read_dir` 遍历后遇到第一个 `.bin.gz`/`.bin`/`.onnx` 就作为当前模型。目录中同时存在轻量、均衡、最强模型时，选择顺序依赖文件系统返回顺序，不一定符合用户选择的 tier。

**影响：** UI 下载某一 tier 后，下一次环境探测可能加载另一模型；分析强度和启动耗时不可预测。

**建议：** 先查与 tier 对应的精确文件名，再使用显式持久化的 selected model；其他模型只作为候选列表显示。

### P2-5：配置文件只在不存在时生成

**位置：** `crates/sabaki-host/src/katago_setup.rs:322-325`

已有 `default_gtp.cfg` 不会根据当前版本或后端重新生成，也没有 schema/version marker。

**影响：** 旧配置可能缺少新版 KataGo 必需字段，或者仍然使用不适合当前机器的 threads/batch 参数；用户只看到握手失败或 unused-config warning。

**建议：** 配置写入带 generator version、backend、schema hash；检测缺失/过期字段后备份并迁移，不能无提示覆盖用户自定义配置。

### P2-6：流式分析每个批次都会走 SGF 持久化路径

**位置：** `apps/sabaki-gpui/src/main.rs:1744-1812`

`push_analysis_batch` 调用 `set_analysis`，`set_analysis` 又调用 `persist_analysis_snapshot`。因此每次约 120ms 的合并批次都尝试把最强候选写入 `SBKV`/`SBKS`。

同时 `parse_lz_analysis_line` 默认把 `is_during_search` 设为 `false`，见 `crates/sabaki-host/src/analysis.rs:41-50`；官方 KataGo GTP `info move` 记录没有 JSON `isDuringSearch` 字段来补充这一状态。

**影响：**

- 中间搜索结果被当成完成结果；
- 会话事件/undo history 可能产生大量属性写入；
- 大棋谱或长时间分析会产生不必要的磁盘和恢复成本；
- “只持久化完成结果”的过滤条件实际上不可靠。

**建议：** 分离 `project_analysis_batch` 与 `commit_analysis_snapshot`：流式期间只更新 UI，收到 stop/drain 完成后只写一次 SGF；或显式给 GTP stream entry 增加 `stream_state`，不要把缺省 `false` 当作完成。

### P2-7：分析 command 解析仍是 whitespace split

**位置：** `apps/sabaki-gpui/src/engine_console.rs:404-424`

`analysis_command_from_settings` 用 `split_whitespace()` 解析 `engines.analyze_commands`。普通引擎参数 parser 支持引号，但分析命令 parser 不支持包含空格的 quoted argument。

**影响：** 代理路径、复杂参数或 future KataGo command value 含空格时会被错误拆分。

**建议：** 复用 `parse_engine_arguments`，再取第一个 token 为命令名。

### P2-8：分析 result 的 best candidate 选择没有统一完成/合法性策略

**位置：** `apps/sabaki-gpui/src/engine_console.rs:364-382`、`main.rs:1768-1774`

best winrate 取最大 visits；持久化也取最大 visits 的非 `is_during_search` entry，但不同 parser 对 `winrate`、player perspective、pass/resign、缺字段的默认值不同。

**建议：** 在 host 定义统一的 completed candidate policy：过滤非法/非有限值、明确 player perspective、明确 pass/resign、按 visits→winrate→稳定 vertex 的 tie-break，UI 与 SGF 写回共用同一策略。

### P2-9：全盘复盘主流程已有，但真实长循环缺少锁定测试

**位置：** `apps/sabaki-gpui/src/main.rs:1953-2037`、`1844-1893`

当前实现正确地保存原节点、按 active lineage 推进、支持自动导航和取消恢复；但现有测试主要是 `BatchReviewState` 小状态测试和 lineage 测试。

尚未形成覆盖以下完整链路的 headless/integration test：

```text
3+ 个真实节点
→ 每节点启动/停止分析
→ 单调 progress
→ 每节点结果绑定
→ 中途取消
→ 恢复 original node
→ stale late result 不写回
```

**建议：** 建立 fake streaming transport + deterministic GPUI dispatcher 测试；再用真实 KataGo 做低 visits 3-node smoke。

### P2-10：安装包不包含 KataGo binary/model，运行时依赖外部环境

**位置：** `scripts/bundle-macos.sh:13-63`

macOS bundle 脚本只复制 `saba-rs` 可执行文件和 plist，没有 KataGo binary、model、config 或明确安装引导。`docs/release-remediation-plan.md:217-219` 也将可执行文件发现和引擎资产目录列为未完成项。

**影响：** 用户从 `.dmg` 安装后仍可能看到“正在连接 KataGo”，但机器没有可执行文件或模型；自动配置链又存在 P0-1 URL 问题。

**建议：** 明确产品策略：随包分发经许可的 engine/model，或首次运行引导用户安装 Homebrew/下载资产；在 UI 中展示缺少的是 binary、model、config 还是权限，而不是统一显示连接失败。

### P2-11：自动 engine role 选择会回退到任意第一个引擎

**位置：** `apps/sabaki-gpui/src/main.rs:931-942`

当没有明确 Analysis role 时，候选先找名字/path 含 KataGo 的记录，否则回退到 `engine_store.list().first()`。

**影响：** 用户只配置 GNU Go 或其他非分析引擎时，点击分析可能自动把它当作 KataGo；错误会在 capability/command 阶段才暴露。

**建议：** 分析 role 只选择声明支持 `kata-analyze`/`lz-analyze` 或用户明确指定的 engine；如果没有匹配项，要求用户选择，不要静默回退。

---

## 5. 按功能域的审查结论

### 5.1 自动安装与环境配置

**结论：部分可用，不可称为完整自动安装。**

模型下载 adapter 和原子替换是合理的，但二进制 URL 失效、没有二进制下载实现、没有 checksum、模型选择非确定、配置升级不完整，因此 setup hub 更接近“模型资源管理 + 本地引擎探测”，不是完整安装器。

### 5.2 GTP handshake/replay

**结论：核心路径可用，但 timeout/reuse 和 fallback framing 仍有高风险。**

真实 KataGo handshake/replay 已有成功证据；然而任何 response timeout 都可能污染 transport，且 fresh stream setup 仍缺少 response consumption。

### 5.3 对弈/genmove

**结论：功能存在，timeout 策略未达到生产稳健性。**

真实 engine smoke 曾成功执行 `play`/`genmove`，但 30 秒硬超时已经在另一轮测试中触发。搜索命令和普通控制命令需要不同的 deadline/取消模型。

### 5.4 普通 bounded analysis

**结论：已后台化，但 stale completion 防护不足。**

bounded analysis 需要复用 streaming run ticket 的 generation/lease disposal 规则，否则 disconnect/new-run 竞争下会出现旧结果回写。

### 5.5 attached streaming analysis

**结论：前台冻结问题已修复，协议状态机仍需加固。**

attached worker 的后台读取和 stop/drain 方向正确；但 GTP timeout、session lease、角色 generation 和异常退出恢复仍可能影响可靠性。

### 5.6 fresh fallback streaming analysis

**结论：可启动但 setup response framing 不完整。**

必须让 replay/setup 每条命令完成 response 校验后再进入 analysis stream。

### 5.7 分析展示与 SGF 写回

**结论：展示链路较完整，持久化时机需要重构。**

候选、PV、winrate、ownership 和 graph 已有 UI；但当前每批流式结果都可能写 SGF，且完成态字段不可靠。

### 5.8 全盘复盘

**结论：状态模型方向正确，真实端到端覆盖不足。**

active lineage、节点绑定、自动推进、取消和恢复原节点均已实现；需要补齐 fake engine 端到端测试和真实 3+ 节点低 visits smoke。

### 5.9 打包与首次运行

**结论：打包主程序可用，但 KataGo 资产策略未闭环。**

`.app`/`.dmg` 不携带 KataGo 资产，也没有可靠的首次运行安装路径。

---

## 6. 测试与验证记录

### 6.1 已执行/已观察

在当前工作树上执行过：

```bash
cargo test -p sabaki-host -q
cargo test -p sabaki-gpui --no-run -q
cargo test -p sabaki-host --test katago_regression -- --nocapture
git diff --check
```

观察结果：

- `sabaki-host` 单元测试通过（当前记录为 138 tests passed）；
- GPUI 测试目标可编译；
- 真实 KataGo regression 在一次运行中 3 项通过；
- 另一轮运行的完整 session smoke 在 `genmove` 处超过 30 秒 timeout；
- 官方 `v1.17.1` release asset API 中没有当前代码所写的 macOS Metal archive 名称；对应 URL 404；
- 三个模型 URL（lightweight、balanced、strongest）可访问，说明模型 URL 与二进制 URL 状态不同。

### 6.2 尚未覆盖的关键测试

1. command timeout 后迟到 response 与后续 command 的 protocol isolation；
2. bounded analyze 在 disconnect/new analysis 期间完成；
3. White/Black/Analysis 三角色并发连接、命令和断开；
4. fresh `AnalysisStream` setup response success/failure/framing；
5. stop 时 child descendant 持有 stdout/stderr；
6. 真实 KataGo 低 visits 的 3+ 节点全盘复盘；
7. 全盘复盘中途取消并恢复 original node；
8. stale late batch 不写入当前节点；
9. 分析结果保存、关闭、重开后的 `SBKV`/`SBKS` 保真；
10. 用户安装 `.dmg` 后 binary/model 缺失时的首次运行体验；
11. Windows/Linux 资产 URL 和可执行文件布局；
12. 模型下载 checksum、损坏 gzip、代理错误、磁盘满和并发下载。
13. 全盘复盘根局面 baseline 与第一手 before/after 评价绑定。
14. 复盘分析失败、EOF、取消和属性写入失败时不得继续推进或显示完成。
15. fallback stream 在 3+ 节点批次中不得每节点重复冷启动 KataGo。
16. 导航后旧候选、PV、ownership 和胜率不应继续投影到新节点。
17. PV、ownership、score drawer 与 SGF 写回必须使用同一最佳候选。
18. 分析命令引号参数、bounded `analyze` 参数和非 KataGo engine capability gating。
19. ownership 长度/范围/非有限值以及 SBKV/SBKS 原子写入。
20. 让子、setup 和非标准 SGF 中 player 必须来自节点属性而不是索引奇偶。

---

## 7. 建议修复顺序

### 阶段 A：先修协议安全与 stale ownership

1. GTP timeout 后 poison/discard transport；禁止复用。
2. 建立 bounded worker 的 ticket/generation gate。
3. 将 `engine_generation` 改为 per-role generation/operation token。
4. 为 lease 增加 token 和 detach/discard intent。
5. 加入 setup command response reader/validator。
6. 用 generation/node/request 绑定的 `PendingAnalysisRequest` 替代 `start_analysis_when_connected: bool`。
7. 在 `sabaki-host` 提取统一 worker seam，例如 `run_analysis(request, source, ticket, emit) -> AnalysisWorkerExit`；attached/fresh 仅作为 source adapter，GPUI foreground 只应用 generation-gated event 和最终 session disposition。

### 阶段 B：修复分析语义与持久化

1. stream setup 和 stream output 使用显式 framing state machine。
2. 把 stream projection 与 SGF commit 分开；stop/drain 完成后只提交一次。
3. 统一 completed candidate、perspective、finite value、pass/resign 和 tie-break policy。
4. 用 `parse_engine_arguments` 解析 analysis command settings。
5. 增加 stale-result、node-binding 和 per-node persistence 测试。

### 阶段 C：修复 KataGo 资产和发布链

1. 修正官方 release asset 映射，或删除错误的二进制下载承诺。
2. 决定 `.app` 是否携带 binary/model，或实现可靠首次运行安装引导。
3. 引入 manifest/checksum/version metadata。
4. 让 model selection 与 tier 确定性绑定。
5. 给 config 增加 schema/version/backend 迁移策略。

### 阶段 D：完善关闭、搜索 timeout 和多角色

1. 命令按类型配置 timeout。
2. 实现 bounded graceful shutdown，再 kill，再有界 join。
3. 使用进程组/job object 处理后代进程。
4. role state 收敛为单一 enum 或 tokenized lease state。
5. 三角色并发回归。

### 阶段 E：发布前证据

每个候选版本至少记录：

- KataGo 版本；
- 模型名称和 SHA-256；
- backend；
- OS/硬件；
- config 内容或 config hash；
- handshake/replay/play/genmove；
- streaming/stop/reconnect；
- full-game review；
- save/reopen analysis properties；
- abnormal exit recovery；
- `.app`/`.dmg` 首次安装和缺资产提示。

---

## 8. 最终发布判断

当前状态建议标记为：

```text
KataGo core path:         可运行
GPUI freeze regression:   主要路径已修复
Real engine smoke:        有通过证据，但 timeout 存在环境敏感性
Whole-game review:        实现完成，端到端测试不足
Auto setup:               不完整，二进制下载 URL 阻塞
Protocol robustness:      未达到发布门槛
Packaging:                主程序可打包，KataGo 资产策略未闭环
Release readiness:        不建议以“完整稳定 KataGo 集成”发布
```

最先应处理的是 **P0-1、P0-2、P1-1、P1-2、P1-3**。其中 P0-2 是最需要优先隔离的基础协议风险：如果 timeout 后仍复用同一 transport，任何上层 cancellation、reconnect 或 analysis 修复都可能被迟到 GTP response 重新破坏。

---

## 9. 范围外但相邻的安全发现

以下问题不属于 KataGo/GTP 本身，因此不计入上面的 KataGo 发布评级；但 KataGo Setup Hub 以插件形式分发，用户可能通过同一个插件安装入口接触该路径，建议另立安全修复任务。

### P1：插件 archive 直接解压到 live 目录，缺少路径与链接安全检查

**位置：** `crates/sabaki-host/src/plugin_workflow.rs:217-259`

`install_plugin_from_zip_file` 创建 live destination 后，在 Unix 调用外部 `unzip -o`，Windows 调用 `tar -xf`，直接把不可信 archive 解压进去。当前没有显式执行：

- canonical path containment preflight；
- `../` traversal 和 absolute path 拒绝；
- symlink/hardlink/device entry 拒绝；
- duplicate path/case-fold collision 拒绝；
- entry count、单文件大小和总展开大小限制；
- staging directory；
- 完整 manifest/entrypoint 校验后的原子 activation；
- 失败后的完整 rollback。

**影响：** 恶意 archive 可能尝试越过插件安装目录、通过链接覆盖其他文件，或在 extraction 失败时留下混合/部分升级文件。后续扫描可能把残留目录当作可授权 native plugin。

**建议：**

1. 使用可审计的 bounded in-process archive reader；
2. normalize 每个 entry path，并确认最终路径严格位于 fresh staging root；
3. 拒绝 absolute、parent traversal、symlink、hardlink、device、duplicate 和大小超限 entry；
4. staging 中完成 extraction 后验证 manifest、plugin id、entrypoint 和权限；
5. 通过 rename 原子激活，升级时保留 rollback；
6. 增加 traversal、absolute path、link、duplicate、case collision、zip bomb、partial extraction 和 partial upgrade 测试。
