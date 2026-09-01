# GPUI UI 对齐计划：接近 Sabaki 原版界面 + 左右侧栏

> **状态（2026-08-31）：已被 `prd.md` 取代。** 本文的「对齐原版 Sabaki 三栏界面」
> 目标已被 PRD 的「Apple 设计系统流星新 UI」取代——UI 最终目标以 PRD 新设计为准
> （见 `architecture-review-2026-08-31.md` §7）。本文仅保留为原版 Sabaki 的
> 行为/布局参考（设置键、分栏默认值、交互路径对照），不再作为 UI 目标文档。

> 目标不是逐像素复刻 Electron DOM，而是以原版 Sabaki 为行为参考，在
> GPUI 原生能力内实现可长期维护的「三栏主布局 + 左侧引擎栏 + 中央棋盘/
> 模式栏 + 右侧胜率/棋谱/注释栏」，并通过相同设置与相同棋谱做到
> 布局、开关、主要交互路径一致。

## 1. 目标界面

参考原版 `src/components/App.js`、`TripleSplitContainer`：

```text
┌────────────────────────────────────────────────────────────┐
│ MainMenu（可隐藏）                                          │
├──────────┬───────────────────────────────┬─────────────────┤
│ 左栏      │ 中央                         │ 右栏             │
│ 250px    │ Goban                        │ 200px           │
│ 默认隐藏 │                              │                 │
│          │                              │ WinrateGraph    │
│ Engines  │                              │ （90px，可拖）   │
│ PeerList │                              ├─────────────────┤
│ 130px    │                              │ Slider +        │
│ 可拖分栏 │                              │ GameGraph       │
│          │                              ├─────────────────┤
│ GTP      │                              │ CommentBox      │
│ Console  │                              │ 注释/评价/标题  │
│          ├───────────────────────────────┤                 │
│          │ 模式栏 PlayBar / EditBar /    │                 │
│          │ ScoringBar / FindBar / ...   │                 │
└──────────┴───────────────────────────────┴─────────────────┘
```

对应原版默认设置：

| 设置 | 原版默认 | 含义 |
|---|---|---|
| `view.show_leftsidebar` | `false` | 左栏默认隐藏 |
| `view.leftsidebar_width` | `250` | 左栏宽度 |
| `view.peerlist_height` | `130` | 左栏引擎列表高度 |
| `view.sidebar_width` | `200` | 右栏宽度 |
| `view.show_graph` | `false` | 右栏 GameGraph 开关 |
| `view.show_comments` | `false` | 右栏 CommentBox 开关 |
| `view.show_winrategraph` | `true` | 胜率图开关 |
| `view.winrategraph_height` | `90` | 胜率图高度 |
| `view.properties_height` | `50` | 右栏 graph/comment 分栏比例 |

原版推断规则：右栏总显隐 = `showGameGraph || showCommentBox`；胜率图独立。

## 2. 参考组件映射

| 原版 Electron/Preact | GPUI 目标实现 | 状态 |
|---|---|---|
| `App.js` | `ShellApp` 根布局 + 窗口/menu/actions | 已有雏形，需重构为三栏 |
| `TripleSplitContainer` / `SplitContainer` | `SplitPane` / 三栏布局，鼠标拖拽 + min + 持久化 | M0 已完成（迭代 28） |
| `LeftSidebar` + `PeerList` + `GtpConsole` | 左栏：引擎会话列表 + GTP 控制台，可上下分栏 | 部分：engine panel 在右侧堆叠 |
| `MainView` + `Goban` | 中央棋盘组件 | M1 已完成：模式栏、坐标类型、路径编号、ghost stones、分析候选标签、线/箭头拖拽 |
| `bars/PlayBar` | 对局栏：黑白棋手、提子、引擎忙碌、Pass/Resign/Estimate/Score/Edit/Find 菜单 | 部分：棋手名/段位、当前手方、Pass/Resign、模式切换已落地；提子数/引擎忙碌待补 |
| `bars/EditBar` | 编辑工具栏 | 部分：markup toolbar 已迁入 Edit 模式 |
| `bars/ScoringBar` | 计分/估算栏与详情 | 部分：Scoring/Estimator 模式栏显示操作提示与计分摘要；详情抽屉未开始 |
| `bars/GuessBar` / `AutoplayBar` / `FindBar` | 猜局/自动播放/查找栏 | 基础：猜测下一着、逐手推进变化、按交点查找 |
| `Sidebar` + `WinrateGraph` | 右栏胜率图，点击跳转手数 | M3 已完成 |
| `Sidebar` + `GameGraph` | 右栏完整游戏树图 | M3 已完成；M4 已补节点右键命令 |
| `Sidebar` + `CommentBox` | 注释编辑 + BM/DO/IT/TE、UC/GW/DM/GB 评价标签 | M3 已完成；M5 使用平台输入桥接 |
| `DrawerManager` + drawers | Info/Score/Preferences/GameChooser/CleanMarkup/AdvancedProperties 抽屉 | M4 完成可用的 Info/Score/Preferences/About；其余等待 host workflow |
| `MainMenu` + `menu.js` | 菜单/快捷键完整矩阵 | M4 已完成 File/Edit/View/Mode/Engines/Navigate/Help |
| `ThemeManager` + themes | 原版主题 CSS/tokens + 明暗调色板 | M5：schema v2 shell semantic colors，v1 主题兼容 fallback |
| `InputBox` / `InputHandler` | 原生文本输入组件 | M5：CommentBox/节点标题接入 EntityInputHandler 与 UTF-16 replacement |

## 3. 非目标 / 延迟项

- 不追求像素级 CSS 一致；不引入 WebView。
- 多人对弈/PeerList 网络能力先以引擎会话列表形式保留，网络协议后置。
- 声音模块在 UI 对齐后单独实现。
- 插件 UI 只使用宿主闭集组件，不开放任意 GPUI 渲染。

## 4. 里程碑与迭代计划

### M0：三栏布局基座（迭代 28，已完成）

**状态：基础三栏、分栏拖拽、显隐按钮与宽度持久化已落地。**
下一步按 M1 继续。

- 实现 `SplitPane`（水平/垂直，可拖拽，min/max，finish 持久化）。
- 实现 `TripleSplit`：左栏 / 中央 / 右栏，宽度来自 `view.leftsidebar_width`、
  `view.sidebar_width`，显隐来自 `view.show_leftsidebar` 与
  `show_graph || show_comments`。
- 迁移现有面板：引擎控制台 → 左栏；插件面板暂留右栏“临时”区；
  棋盘列 → 中央；变化树/注释/引擎等逐步归位。
- 持久化拖拽结果到 `view.*_width` / `view.*_height` 设置键。
- 测试：`frontend_smoke` 增加左/中/右三栏 bounds、拖拽后宽度、
  显隐开关回归。

**验收：**
- 隐藏左右栏时中央棋盘可用且不裁切。
- 拖分栏后重启宽度恢复。
- 默认设置下界面与原版显隐状态一致。

### M1：中央棋盘与模式栏对齐（迭代 29-30，已完成）

**状态：迭代 29 已落地 Play/Edit/Scoring/Estimator 模式栏、坐标类型与
next/sibling ghost stones；迭代 30 已完成以下第二半项目。**

- Goban 渲染补齐：
  - `view.coordinates_type`（A1 / 1-1）✅
  - `view.move_numbers_type`（start / variation / hotspot）✅
  - `view.show_move_colorization`、`view.show_next_moves`、`view.show_siblings` ✅
  - 悬停 ghost stone / 下一步提示 ✅
  - 分析候选项、最佳着、胜率标签 ✅
  - 线/箭头拖拽绘制 ✅
- 模式系统：`play / scoring / estimator / edit / find / guess / autoplay`，
  由 ShellApp 状态或 host DTO 驱动；Find、Guess、Autoplay 已提供最小交互。
- 模式栏：
  - `PlayBar`：黑白棋手名/段位、提子数、引擎忙碌、Pass/Resign/
    Estimate/Score/Edit/Find 弹出菜单
  - `EditBar`：现有 markup toolbar 迁入
  - `ScoringBar`：结果、详情按钮、dead stone 提示
  - `FindBar`、`GuessBar`、`AutoplayBar`：已提供查找、猜着、逐手推进入口

**验收：**
- 打开原版常用 SGF，棋盘显示项与设置逐项一致。
- 每种模式的底部栏切换与键盘/菜单入口一致。
- Pass/Resign/Score/Edit/Find 至少各有 GPUI 入口。

### M2：左栏引擎区对齐（迭代 31，已完成）

**状态：** 左栏已分成高度可持久化的 engine roster 与 GTP Console。角色通过
持久化的 `engines.analysis`、`engines.black`、`engines.white` 保存配置引擎名称，
并分别拥有独立的真实 GTP session；同一配置可安全充当多个角色。

- 左栏上下分栏：
  - 上：引擎列表，显示 attach/detach、当前选择和 analysis/black/white 角色。
  - 下：GTP Console，显示日志与当前选中引擎的命令输入。
- 支持选择分析引擎、黑白对弈引擎；每个角色单独 attach/detach、同步局面与
  生成着。console 只路由到 active role，未 attach 时明确拒绝而不会误用其他 session。
- 流式 Analysis session 以 generation 租借：局面变动会停止并重放，detach/换局/
  重载后旧 worker 不会把已释放 session 放回。
- 输入继续复用当前 GPUI focus + keydown 控件；完整原生文本输入（IME、选区、
  剪贴板、undo）与 M5 合并实现，避免在此重复维护一个不完整输入组件。
- `view.peerlist_height` 拖拽持久化，`frontend_smoke` 覆盖初始高度、拖拽和落盘。
- 左栏通过 `Engines > Show Engines Sidebar`、`Cmd/Ctrl+Shift+B` 或
  `view.show_leftsidebar` 切换。

**验收：**
- fake-gtp-engine 的既有真实进程 smoke 可用于手工检查 attach、角色、日志与命令。
- `cargo test -p ryusei-gpui` 覆盖角色分配、三栏/左栏 bounds 及纵向分栏持久化。
- 与真实 KataGo/GNU Go 的手工验证仍需用户配置引擎和模型。

### M3：右栏胜率图 + 完整游戏树图 + 注释框（迭代 32-33，已完成）

**状态：** 右栏现在拥有独立、可持久化的 WinrateGraph 与 properties 分栏；所有
图形状态来自 host snapshot，不拥有也不操纵任何 GTP session。

- `WinrateGraph`：当前变化路径的 `SBKV`/`SBKS` 与 live fallback 形成历史；支持
  winrate/score-lead、反转、阈值败着标记，以及按横轴任意位置跳转手数。已完成的
  Analysis-role 结果以 generation、请求节点和执棋方门控写回 `SBKV`/`SBKS`。
  `view.winrategraph_height`、反转和两个阈值均经 SettingsStore 持久化。
- `GameGraph`：使用无碰撞 matrix layout，主线沿行、变体保留右侧子树列；当前路径、
  注释/评价颜色、pass/setup/hotspot 形状及 `graph.grid_size`/`graph.node_size` 均已
  落地。双轴 scroll viewport 代替原版 camera，节点左键经 host transaction 跳转。
  右键菜单属于 M4 popup/menu 工作。
- `CommentBox`：支持注释、`N` 节点名、BM/DO/IT/TE、UC/GW/DM/GB 与独立 HO；同组
  注释自动互斥，`view.show_comments` 控制显示。
- `view.properties_height` 与 `view.winrategraph_height` 的两个内部 splitter 均持久化。

**验收：**
- `frontend_smoke` 验证右栏四个区域/两个 splitter 的 bounds、CommentBox 评价控件，以及
  GameGraph root 的真实点击导航。
- `winrate_graph` 测试覆盖视角转换、SGF 百分比归一化、阈值/反转、横轴定位及写回值；
  分析后历史可由 SGF `SBKV`/`SBKS` 重建。

### M4：抽屉、菜单与设置补全（迭代 34-35，已完成）

**状态：** 已以当前 host 的真实能力完成可用 drawer 和原生菜单覆盖；不将缺少
host workflow 的多棋谱选择、批量 markup 清理或任意 SGF property 编辑伪装成空抽屉。

- Drawer 体系：
  - InfoDrawer：棋盘、手数、贴目、棋手、结果和来源文件。
  - ScoreDrawer：确定性 score summary 与 override 数量。
  - PreferencesDrawer：设置表单已从右栏迁入，SettingsStore/persistence 行为不变。
  - About drawer：客户端/host 架构身份。
  - GameChooser、CleanMarkup、AdvancedProperties 需先扩展 host 的多文档、批量事务和
    property workflow，列入后续 host milestone。
- MainMenu 现覆盖 File/Edit/View/Mode/Engines/Navigate/Help：View 管理三个右栏开关和
  drawers；Mode 直接切换所有已实现 `GameMode`；Engines 可开关侧栏和启动/停止 analysis。
- `view.show_menubar` 在下次启动时决定是否安装 native menu。`Cmd+,` 打开 Preferences，
  `Cmd+1..4` 切换 Play/Edit/Scoring/Estimator。
- GameGraph 节点右键提供跳转、切换 HO hotspot 与关闭命令，写入仍通过 host transaction。

**验收：**
- `frontend_smoke` 验证 Preferences、Game Info、Score drawer 的打开/关闭、M3 右栏 bounds，
  以及 GameGraph 右键 HO 修改。
- 菜单结构测试锁定 File/Edit/View/Mode/Engines/Navigate/Help；package/workspace 测试
  保护 M0–M3 行为。

### M5：输入、主题、声音与发布门槛（迭代 36-37，已完成）

- `NativeTextInput`：Unicode character selection、undo/redo、`Cmd/Ctrl+A/Z/Shift+Z`、
  UTF-16 replacement。CommentBox 与节点 `N` 标题以 `NativeInputBinding` 在 paint 时安装
  `ElementInputHandler<ShellApp>`；Enter/Escape 的 host transaction 边界不变。
- Theme schema v2：host 验证可选 `shell` semantic color group（drawer/input/status/graph track）；
  Classic/Dark/Mist 提供显式 v2 tokens，第三方 schema v1 主题继续按背景派生 fallback。
- Sound：`sound.enable` 控制 UI-local `SoundSink`，且仅在人工、引擎和 pass 的 `play_move`
  成功后发 cue。默认 `NoopSoundSink` 刻意不绑定平台音频设备；捕子/终局 cue 等待 host
  暴露相应语义事件，不能由 UI snapshot 猜测。
- Beta 证据：`frontend_smoke` 验证实际 GPUI layout/input bindings 的安全渲染，135 个 GPUI
  测试及全 workspace test/doc-test 完整通过；现有 TestAppContext 是等价 headless GPU CI
  证据。Electron 并行人工 UI 差异清单仍属于发布前人工 QA，不伪造为自动化对比。

**验收：**
- 用户可完成开棋、对弈、分析、注释、计分和保存；M0–M4 行为由全量 workspace 回归保护。
- 剩余发布前工作是平台音频 backend 与 Electron 手工并行 QA，并非 host/domain 行为缺口。

## 5. 技术依赖与风险

| 依赖/风险 | 影响 | 策略 |
|---|---|---|
| gpui 0.2.2 无成熟 SplitContainer/拖拽 | 三栏布局要自研 | 先移植最小 SplitPane 状态机（参考 §7 的 `gpui-component::resizable`、`open-gpui/ui_components/splitter.rs`），并用 `frontend_smoke` 做拖拽测试 |
| 无 popup/context menu 现成组件 | PlayBar 菜单、节点右键菜单 | 优先用 GPUI `Menu`/action；若不足则自研 lightweight popup |
| 无 canvas-like 高性能图组件 | GameGraph/WinrateGraph | MVP 用绝对定位 Div，数据量超阈值后下沉 custom `Element` paint |
| 分析结果写入 SGF 会产生 dirty 状态 | 胜率历史与未保存提示增加 | 仅写入完成候选，使用 generation/node/player 门控，并复用 host recovery |
| `ShellApp` 状态继续膨胀 | 迭代 27 后仍集中 | 随 M0-M3 拆 `layout_state`/`mode_state`/`sidebar_state` |
| 自定义文本输入不完整 | IME/剪贴板 | M5 前迁移 `TextElement`/`EntityInputHandler` |
| 真实引擎验证缺环境 | 左栏引擎流程 | fake-gtp 冒烟先行，用户环境手工验证 |

## 6. 每个迭代的通用验收

1. `cargo test --workspace` 全绿。
2. `frontend_smoke` 覆盖本迭代新增布局/交互。
3. 更新 `docs/handoff.md` 与 `docs/beta-gate.md`。
4. 提交推送，`gh run` 三平台 CI 全绿。
5. 不引入 WebView/Electron/Node 依赖。

## 7. 可借鉴项目调研（2026-08 查询）

经 `gh search` 与本地 clone 核对，当前生态已有可直接参考的实现，不需要从零发明：

### 7.1 首选参考：longbridge/gpui-component（约 12.8k stars）

- 仓库：`https://github.com/longbridge/gpui-component`
- 定位：基于 GPUI 的 shadcn 风格组件库，分两层：
  - `gpui-component`：带样式的完整组件
  - `gpui-base`：无样式的基础行为/状态/焦点/主题设施
- 与本项目直接相关：
  - `crates/ui/src/dock/`：`DockArea` 原生支持 **left/right/bottom dock + center**，
    有 `DockAreaState`/`PanelState` 可序列化恢复，适合 M0 三栏布局；
  - `crates/ui/src/resizable/` / `gpui-base::ResizablePanelGroup`：可拖拽分栏；
  - `crates/ui/src/sidebar/`：sidebar 显隐、折叠、分组菜单；
  - `crates/ui/src/chart/`：line/area/bar/pie 等图表，WinrateGraph 可优先移植；
  - `crates/ui/src/input/`：`InputState`/textarea/选区/弹层，解决 M5 输入问题；
  - `crates/ui/src/menu/`：popup/dropdown/context menu；
  - `crates/ui/src/dialog/`、`sheet.rs`、`popover.rs`：Drawer 可参考；
  - `crates/ui/src/setting/`：设置面板；
  - `crates/ui/src/theme/`：主题 token schema，可参考做 theme schema v2。
- 关键注意：
  - 它依赖 `gpui = { version = "0.2.2", git = "https://github.com/zed-industries/zed" }`
    与 `gpui_platform`。我们当前固定 crates.io `gpui = "0.2.2"`，**直接添加依赖
    很可能出现两个 GPUI 源码包、类型不兼容**。
  - 因此建议：M0 只借鉴其 SplitPane/DockState 设计并移植最小实现；如要整体
    采用，需先在一个 spike 分支把本项目 GPUI 依赖切到同一 git revision，
    验证三平台 CI、macos-blade、`font-kit` 后再决定。

### 7.2 社区分栏/停靠参考

| 项目 | Stars | 可借鉴点 | 注意 |
|---|---|---|---|
| `Latias94/open-gpui` | 见仓库 | `crates/ui_components/src/splitter.rs`、`sidebar.rs`、`text_input.rs`、`slider.rs`、`context_menu/`；`crates/gpui_docking` 是完整的 retained docking 系统，含持久化、tab stack、分栏、测试 | 它是 GPUI 的 fork（`open-gpui` 0.2.0），不能直接依赖，只作实现参考 |
| `ignition-is-go/gpui-mullion` | 见仓库 | 轻量 split panes + activity bar；`model`/`tree`/`command_actions` 把 pane 拓扑、分栏比例、resize 命令拆成纯逻辑，适合我们 M0 的“可测试状态机”设计 | 依赖 zed git GPUI，版本较新 |
| `ColinEspinas/jerry` | 较小 | `crates/app/src/sidebar` 提供简洁 sidebar 实例 | 仅模式参考 |

### 7.3 官方/上游参考

- `zed-industries/zed`：GPUI 本体。重点目录：
  - `crates/workspace/src/dock.rs`：Zed 的 dock 布局；
  - `crates/sidebar/src/sidebar.rs`：产品级 sidebar；
  - `crates/ui/src/components/`：官方 UI 组件；
  - `crates/gpui/examples/`：本地 cargo registry 已有 `data_table.rs`、
    `drag_drop.rs`、`scrollable.rs`、`input.rs`、`tree.rs`、`uniform_list.rs`
    等，可作为不引第三方时的最稳妥写法。
- `zed-industries/awesome-gpui`：GPUI 项目索引，后续可继续按“dock/sidebar/chart”
  检索新出现的成熟项目。
- 已确认可参考的成熟应用：`Zedis`（Redis GUI）、`Loungy`、`Waku`、
  `OpenLogi`、`oxideterm`、`hummingbird`；它们可验证 GPUI 在列表、终端、
  输入、图表等场景的实际做法。

### 7.4 采纳策略

1. **M0 采用“读码移植”而非直接引依赖**：只移植 SplitPane/DockArea 的
   状态模型与命中测试，保持本项目 `gpui = 0.2.2 crates.io` 不变。
2. **M1-M4 逐组件评估**：优先移植最小可用实现；若某组件超过阈值（例如
   WinrateGraph、TextInput、Menu），再评估引入/照抄 gpui-component 对应模块。
3. **切换 GPUI 源作为独立 spike**：
   - 分支：`spike/gpui-component-adoption`
   - 验证：`cargo tree` 无重复 gpui、三平台 CI、`macos-blade`/`font-kit`、
     `test-support`、release 打包。
   - 通过后再决定是否把 `gpui`/`gpui-platform` 切到 zed git 并启用
     `gpui-base` 或 `gpui-component`。
4. **不切换到 open-gpui fork**：除非 zed 上游停更且社区 fork 成为事实标准；
   本项目目标是 Sabaki 产品，不应承担 fork 框架的升级成本。
