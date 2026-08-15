# GPUI UI 对齐计划：接近 Sabaki 原版界面 + 左右侧栏

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
| `MainView` + `Goban` | 中央棋盘组件 | 部分：goban_view 可点击/标记/分析点 |
| `bars/PlayBar` | 对局栏：黑白棋手、提子、引擎忙碌、Pass/Resign/Estimate/Score/Edit/Find 菜单 | 部分：仅 Pass 按钮 |
| `bars/EditBar` | 编辑工具栏 | 部分：markup_toolbar |
| `bars/ScoringBar` | 计分/估算栏与详情 | 部分：设置面板里有计分摘要 |
| `bars/GuessBar` / `AutoplayBar` / `FindBar` | 猜局/自动播放/查找栏 | 未开始 |
| `Sidebar` + `WinrateGraph` | 右栏胜率图，点击跳转手数 | 未开始 |
| `Sidebar` + `GameGraph` | 右栏完整游戏树图 | 未开始（当前只有 variation_tree） |
| `Sidebar` + `CommentBox` | 注释编辑 + BM/DO/IT/TE、UC/GW/DM/GB 评价标签 | 部分：node_inspector 注释编辑 |
| `DrawerManager` + drawers | Info/Score/Preferences/GameChooser/CleanMarkup/AdvancedProperties 抽屉 | 未开始 |
| `MainMenu` + `menu.js` | 菜单/快捷键完整矩阵 | 部分：File/Edit/Navigate 基础菜单 |
| `ThemeManager` + themes | 原版主题 CSS/tokens + 明暗调色板 | 部分：UiPalette 已落地 |
| `InputBox` / `InputHandler` | 原生文本输入组件 | 未开始（当前为 track_focus + keydown） |

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

### M1：中央棋盘与模式栏对齐（迭代 29-30）

**目标：中央区域达到原版 MainView 的主要行为。**

- Goban 渲染补齐：
  - `view.coordinates_type`（A1 / 1A）
  - `view.move_numbers_type`（start / current）
  - `view.show_move_colorization`、`view.show_next_moves`、`view.show_siblings`
  - 悬停 ghost stone / 下一步提示
  - 分析候选项、最佳着、胜率标签
  - 线/箭头拖拽绘制（当前只有点击标记）
- 模式系统：`play / scoring / estimator / edit / find / guess / autoplay`，
  由 ShellApp 状态或 host DTO 驱动。
- 模式栏：
  - `PlayBar`：黑白棋手名/段位、提子数、引擎忙碌、Pass/Resign/
    Estimate/Score/Edit/Find 弹出菜单
  - `EditBar`：现有 markup toolbar 迁入
  - `ScoringBar`：结果、详情按钮、dead stone 提示
  - `FindBar`、`GuessBar`、`AutoplayBar`：MVP 可后置但预留 slot

**验收：**
- 打开原版常用 SGF，棋盘显示项与设置逐项一致。
- 每种模式的底部栏切换与键盘/菜单入口一致。
- Pass/Resign/Score/Edit/Find 至少各有 GPUI 入口。

### M2：左栏引擎区对齐（迭代 31）

- 左栏上下分栏：
  - 上：引擎列表（黑白/分析引擎角色、连接状态、busy）
  - 下：GTP Console（当前 engine panel 的 transcript/输入）
- 支持选择分析引擎、黑白对弈引擎；引擎 attach/detach 状态清晰。
- 控制台输入迁移到原生文本输入组件。
- `view.peerlist_height` 拖拽持久化。
- 左栏通过菜单 `View > Show Engines Sidebar` / `view.show_leftsidebar` 切换。

**验收：**
- 连接 fake-gtp-engine 后左栏显示引擎、角色、日志，命令可发送。
- 与真实 KataGo/GNU Go 的手工验证清单可执行。

### M3：右栏胜率图 + 完整游戏树图 + 注释框（迭代 32-33）

- `WinrateGraph`：
  - 数据源：当前分析流 + SGF 节点 `SBKV`/`SBKS` 持久化
  - 折线/面积、当前手高亮、点击跳手、败着阈值着色
  - `view.winrategraph_height` 拖拽持久化
- `GameGraph`：
  - 替代当前 variation_tree，完整变化树、当前路径、兄弟分支、网格/
    节点大小设置 `graph.*`
  - 左键跳节点，右键节点菜单
  - 自动滚动/滑块
- `CommentBox`：
  - 注释编辑 + N 节点名 + BM/DO/IT/TE、UC/GW/DM/GB 评价
  - `view.show_comments` 控制
- 右栏内部 `graphproperties` 与 comment 分栏比例 `view.properties_height`。

**验收：**
- 多分支教学谱可缩放/点击/跳转，当前路径与 sibling 清晰。
- 分析后胜率图可点击回跳，刷新后数据仍在（SBKV/SBKS）。

### M4：抽屉、菜单与设置补全（迭代 34-35）

- Drawer 体系：
  - InfoDrawer（对局信息）
  - ScoreDrawer（计分详情）
  - PreferencesDrawer（现有设置面板迁入抽屉或保留右栏）
  - GameChooserDrawer（多棋谱管理）
  - CleanMarkupDrawer / AdvancedPropertiesDrawer
- MainMenu 对齐 `menu.js` 的 File/Edit/View/Mode/Engine/Help 结构。
- `view.show_menubar` 生效。
- 设置面板不再堆叠在右栏，改为 Preferences 抽屉；
  所有暴露设置均有效或明确禁用。
- 快捷键矩阵补齐。

**验收：**
- 原版主菜单中每项常用动作均有 GPUI 等价入口。
- 所有 Drawer 打开/关闭/提交无阻塞。

### M5：输入、主题、声音与发布门槛（迭代 36-37）

- 原生文本输入组件：光标/选区/剪贴板/IME/undo。
- Theme schema v2：材质/尺寸 token；自定义主题与 Dark/Mist 对比度回归。
- Sound 模块：落子/提子/结束音；`sound.enable` 重新进入设置面板。
- Beta #10：完成 screenshot/offscreen 方案或等价 GPU CI 证据。
- 与 Electron 原版并行跑同一 SGF fixture，生成 UI 差异清单。

**验收：**
- 用户可在不读日志的情况下完成开棋、对弈、分析、注释、计分、保存。
- Beta 门槛十项全部满足。

## 5. 技术依赖与风险

| 依赖/风险 | 影响 | 策略 |
|---|---|---|
| gpui 0.2.2 无成熟 SplitContainer/拖拽 | 三栏布局要自研 | 先移植最小 SplitPane 状态机（参考 §7 的 `gpui-component::resizable`、`open-gpui/ui_components/splitter.rs`），并用 `frontend_smoke` 做拖拽测试 |
| 无 popup/context menu 现成组件 | PlayBar 菜单、节点右键菜单 | 优先用 GPUI `Menu`/action；若不足则自研 lightweight popup |
| 无 canvas-like 高性能图组件 | GameGraph/WinrateGraph | MVP 用绝对定位 Div，数据量超阈值后下沉 custom `Element` paint |
| 分析数据当前不落 SGF | 胜率图刷新后丢失 | 扩展 domain SGF `SBKV`/`SBKS` 读写 |
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
