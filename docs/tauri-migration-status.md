# Tauri 重构实施状态

> 最后审查：2026-08-12。
>
> **最新状态（2026-08-14）：** 本文为历史迁移记录。Tauri 已**永久冻结为行为参考**，
> `apps/sabaki-gpui` 已从 spike 升级为正式 GPUI 主客户端并成为唯一开发主线。
> 最新架构快照见 [`handoff.md`](handoff.md)。此后 Tauri 侧不再新增开发；
> host 侧新能力（设置键表、引擎工作流与会话、插件注册表、全部持久化边界）均由
> GPUI 客户端消费。
>
> 本文记录 Rust + Tauri 迁移的已实现范围、验证状态与已知缺口。它是
> [`tauri-rearchitecture-design.md`](tauri-rearchitecture-design.md)
> 的实施状态补充；架构目标以设计文档为准，交付顺序以
> [`tauri-migration-next-steps.md`](tauri-migration-next-steps.md) 为准。
>
> Electron 版本仍是正式发行版、行为参考和迁移期间的回退路径。当前 Tauri 实现是一个可构建、可测试的迁移切片，而不是功能完整的替代品。
>
> **迁移主线说明：** 最终原生客户端主线已确定为 **Rust +
> GPUI**（`apps/sabaki-gpui`，原 spike 已升级为正式客户端）。Tauri/Preact
> 只保留为过渡 adapter、UI 行为参考和回退切片，不再是最终客户端架构；从
> `sabaki-host` 到两端 UI 的所有连接都以 typed ports 为边界，GPUI 与 Tauri
> adapter 复用同一套 ports。参见
> [`tauri-rearchitecture-design.md`](tauri-rearchitecture-design.md)
> 的最新架构章节与 [`handoff.md`](handoff.md)。

## 1. 执行摘要

当前实现已经建立了受限的 Rust/Tauri 边界，并完成第一个可编辑棋盘的端到端垂直切片：Preact
UI 通过异步 Tauri command/event
bridge 操作 Rust 领域游戏树，而不是访问 Electron、Node API 或可写游戏树对象。

已实现的核心能力包括：完整 SGF 变化树的解析与序列化、棋局树事务、棋盘快照、落子/导航/撤销重做、注释与标记编辑、变化提升/删除，以及初步的插件 manifest和原生 JSON-RPC 框架。

尚未达到 Beta 门槛。最大的阻断项是：共享差分 fixture 覆盖不足、旧格式与更广泛 SGF 编码兼容性尚未硬化、设置/主题 UI 与跨版本默认值策略尚未完成、文件工作流仍缺原生 e2e，以及 GTP 服务、完整 UI、WASM 执行环境、跨平台 CI 尚未完成。

## 2. 已实现范围

### 2.1 Rust 领域核心

`crates/domain-core` 已提供：

- 版本化 `GameDocument`
  游戏树：稳定节点 ID、父子关系、当前节点、首选子变化、undo/redo、文档 revision 与保存状态；
- 完整 SGF 树解析、嵌套变化、转义、多值属性、压缩点与未知属性保留；
- 走子、提子、自杀检查、简单劫争、虚手、setup stones、矩形棋盘和棋盘重建；
- `BoardSnapshot`：棋子、标记、箭头/线条、子变化/同级变化覆盖、手番和手数；
- 命名事务：`playMove`、`pass`、`setNodeProperty`、`removeNodeProperty`、
  `addMarkup`、`appendVariation`、`removeVariation`、`promoteVariation`、
  `navigate`；
- 版本化、camelCase 的前端 DTO；
- GTP 命令/响应解析与基础进程传输类型。

`applyScoringOverride` 已在 schema 中预留，但尚未实现；当前并不具备计分模式。

### 2.2 Tauri 宿主

`src-tauri` 已实现最小宿主层：

- 新建、读取快照、落子、通用事务、撤销和重做 commands；
- 宿主拥有的 SGF 打开、保存和另存为 commands；打开/另存为由原生对话框选择路径，取消不会替换当前文档；
- 可独立测试的 `file_service` 与 `game_workflow_service`：依据 `CA`
  声明以严格白名单解码 UTF-8、Shift_JIS、EUC-JP、GBK 和 Big5；未知、无效或需 replacement 的编码均失败而不触碰来源文件；保存沿用当前来源编码，若编辑内容无法无损表示则拒绝写入，不会替换来源文件。原子写入仍使用默认
  `.sgf` 扩展名、同目录唯一临时文件和原子替换；`game_workflow_service`
  提供可注入的文件访问边界，`host_persistence_service`
  将 recovery/recent-files 的加载、持久化和清理约束为可注入的
  `HostPersistence`，其失败路径会回滚内存 store；
- `game-state-changed` 完整快照事件；
- 宿主拥有的窗口关闭确认：dirty 文档会由原生对话框确认丢弃，清洁文档直接关闭；
- 安全的旧设置/用户样式导入：宿主原生 JSON 选择器、已知键/类型校验、未知键与无效值报告、不可变备份、单次迁移标记及可恢复的多文件写入；
- 设置快照启动加载与受校验、可恢复的持久化写入；
- 宿主拥有的最近文件 registry：独立 `recent-files.json`
  持久化、最多 10 条、路径去重、启动加载、host 侧缺失检测；前端只接收不透明 ID、显示名和缺失状态，并只能以 ID 请求重新打开；
- 宿主拥有的 crash-recovery 自动保存：每次成功的 dirty 文档变更都会原子写入独立
  `recovery.json`；恢复记录只在宿主存储 SGF 和路径显示名，UI 仅接收可用性/时间/revision/显示名 DTO。启动时必须明确恢复或丢弃，恢复后得到无来源路径的 dirty 文档，因而 Save 必定进入 Save
  As，永不写回原始 SGF；成功显式保存会清除恢复记录；
- 宿主拥有的外部 SGF 修改检测：host 在成功打开/保存后以 SHA-256 内容指纹跟踪来源文件；窗口重新获得焦点时以受限 command 请求检查。clean 文档遇到可解析变更会由 host 自动重载，dirty 文档绝不被静默替换，而是进入冲突状态。来源缺失/不可读或 dirty 冲突会阻止常规 Save，用户只能显式 Reload 或“保留本地修改”并进入无路径 dirty 文档后的 Save
  As；UI 仅接收状态和显示名，不接收新的来源路径或文件内容；
- 内置主题清单读取；
- 插件列表、安装、启用及原生执行授权 commands；
- SGF、NGF、GIB、UGF 的文件关联声明。

当前 `main.rs`
仍是装配与服务逻辑混合的单文件实现。当前 UI 使用的 game/file/settings
command 已返回稳定的
`CommandErrorDto`（code、message、可选 details）；自动保存和外部修改的 game/file
commands 已采用该模型；窗口状态、菜单、快捷键，以及 theme/plugin 服务的错误 DTO 尚未实现。

### 2.3 Preact 迁移 UI

`src/tauri` 已完成不依赖 Electron/Node 的独立 UI 切片：

- `bridge.js`：受限 Tauri
  command/event 调用，保留结构化 host 错误的 code、message 和 details；
- `store.js`：初始化、host
  event 订阅、命令串行化、忙碌状态与 revision 过期快照丢弃；
- `Goban`：通过 Shudan 渲染 Rust `BoardSnapshot`；
- `NavigationBar`：首/前/后/末节点导航、undo/redo、新建棋局和未保存替换确认；
- `FileActions`：宿主对话框打开、保存、另存为、host-owned 最近文件菜单及 dirty 状态指示；
- 启动恢复提示：发现 host-owned 自动保存后，用户必须选择恢复或丢弃；恢复的文档没有来源路径并保持 dirty，因而只能显式 Save
  As；
- 外部文件冲突提示：窗口聚焦时只请求 host 检查；dirty 冲突提供 Reload 与保留本地修改/Save
  As，clean 变更由 host 自动重载；
- `VariationTree`：可选择节点的 SVG 变化树；
- `NodeInspector`：评论、热点、手法评价、局势评价、属性显示以及变化提升/删除；
- `MarkupToolbar`：圆、方、三角、叉和标签标记。

当前 UI 是迁移预览入口，已包含打开/保存操作，但尚未包含棋谱信息编辑、setup
stone 工具、箭头/线条工具、计分、偏好设置、主题管理、引擎控制台或窗口菜单。

### 2.4 插件基础

`crates/plugin-runtime`
已提供 manifest 校验、权限状态、原生执行的显式授权、长度前缀 JSON-RPC 消息帧与原生进程基础类型。仓库包含 WASM 形态和 Go 原生插件的示例。

尚未提供 WASM runtime、capability
imports、声明式贡献渲染、插件私有存储、完整的原生进程监督或可供第三方依赖的 SDK。

### 2.5 UI 无关的 `sabaki-host` 抽取（GPUI 主线）

已新增
`crates/sabaki-host`，作为 UI 无关的应用工作流 crate，不依赖 Tauri、GPUI、浏览器或窗口 API。它复用
`sabaki-domain-core`，并定义 host 拥有的 typed ports 与 typed events：

- `GameFileAccess`：`read_game_file` / `write_game_file`，携带 `DecodedGameFile`
  与 `SourceEncoding`（UTF-8 / Shift_JIS / EUC-JP / GBK / Big5）；
- `HostEventSink`：`emit(HostEvent)`；事件包括
  `HostEvent::GameChanged { snapshot }`、`AutosaveChanged { info }`、
  `ExternalFileStatusChanged { status }`；
- `HostPersistence`：`load_autosave` / `persist_autosave` / `clear_autosave` /
  `load_recent_files` / `persist_recent_files`，并包含 `synchronize_autosave` 与
  `record_recent_file` 工作流助手（失败时回滚内存 store）；
- `ExternalFileReader`：`read_game_file`，区分 `Missing` / `Unreadable`；
- `HostApplication`：`new` / `create_new` / `open` / `open_decoded` / `save` /
  `save_at` / `play_move` / `apply_transaction` / `undo` / `redo` /
  `discard_source_location` / `restore_from_sgf` / `snapshot` /
  `source_encoding` / `to_sgf`；
- `HostError`：`NoSaveLocation`、`FileRead`、`FileWrite`、`Domain`。

**迭代 H 已完成（宿主状态迁移与 typed events 扩展 + 服务级 workflow 测试）：**

- `sabaki-host` 新增
  `autosave`、`recent_files`、`external_file`、`persistence`、 `close_flow`
  五个 UI 无关模块；autosave
  recovery、recent-files 注册表、外部文件 SHA-256 内容指纹/变更决策的状态机，以及窗口关闭决策全部迁入 host；
- `HostEvent` 扩展为
  `GameChanged`、`AutosaveChanged`、`ExternalFileStatusChanged`；
- Tauri 侧 adapter 保留原生持久化与读取：`autosave_service` /
  `recent_files_service` 只负责 app config 目录的文件读写（复用 `file_service`
  原子写入），`external_file_service` 提供 `NativeExternalFileReader`；
  `host_persistence_service` 的 `NativeHostPersistence` 实现 host 的
  `HostPersistence` trait，并 re-export host 的 `synchronize_autosave` /
  `record_recent_file`；`close_flow` 由 host 提供，Tauri `main.rs` 直接消费；
- `TauriHostEventSink` 把三个 host event 映射为既有的 `game-state-changed`、
  `autosave-changed`、`external-file-status-changed` Tauri events；
- 旧 `SgfFileAccess::write_sgf` seam 已移除（写入统一经
  `sabaki_host::GameFileAccess`）；
- 新增 `crates/sabaki-host/tests/workflow.rs` 服务级组合断言：崩溃恢复 → Save
  As（不覆盖原文件）、clean 外部变更自动重载并重建 baseline、dirty 外部冲突 →
  keep local → Save
  As、恢复/autosave 状态经关闭决策 gate 后仍保留、recent-files 经 persistence
  port 记录与解析。

`sabaki-host`
的确定性内存端口测试覆盖：打开→编辑→保存→重开、读/解析失败不替换当前文档、无保存路径的常规保存拒绝、丢弃来源位置、恢复文档标记、SGF 序列化 round-trip、host落子、autosave 持久化/清除/失败回滚、recent-files去重/缺失报告/上限、外部文件变更检测/clean 自动重载/dirty 冲突/解绑，以及上述服务级 workflow 组合。

## 3. 验证状态

已具备以下自动验证层：

| 层次                    | 当前状态                                                                                                                                                                                                                                                                                                   |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| JavaScript 行为特征测试 | 已覆盖游戏树、编辑命令序列、棋盘变换、分析解析与旧格式 fixture。                                                                                                                                                                                                                                           |
| Rust 单元/集成测试      | 已覆盖游戏树、SGF round-trip、`CA` 白名单解码与无损 legacy 写回拒绝、编码失败不修改源文件、可注入文件访问 boundary、文件服务原子保存与重开、dirty 窗口关闭决策、恢复 Restore/Discard gate 生命周期、外部文件变更决策/reload 后重建 baseline/dirty 冲突后解除来源身份、事务、棋盘覆盖、GTP 解析和插件 RPC。 |
| 共享差分 fixture        | 已覆盖线性主变化、提子、矩形棋盘、setup/标记、变化/历史及 Rust 保存的文件工作流 artifact；Rust 与 JavaScript 两侧均驱动。                                                                                                                                                                                  |
| Tauri 前端单元测试      | 已覆盖 DTO、结构化 command error、store、文件工作流取消/保存分派、外部冲突重载/保留本地修改与状态刷新、棋盘 adapter、导航、变化树、标记和节点元数据。                                                                                                                                                      |
| 前端生产构建            | `npm run tauri:frontend:build` 可通过。                                                                                                                                                                                                                                                                    |
| Tauri e2e               | 尚未开始；现有 e2e 仍以 Electron 为目标。                                                                                                                                                                                                                                                                  |
| 性能基准                | 尚未开始。                                                                                                                                                                                                                                                                                                 |
| Rust/Tauri CI           | 部分完成                                                                                                                                                                                                                                                                                                   | Ubuntu CI 已执行 Rust 格式检查和 workspace tests；跨平台 Tauri 构建、签名、更新与平台 e2e 仍未接入。 |

当前切片已通过的本地检查命令：

```bash
npm test
npm run format-check
npm run tauri:frontend:build
cargo test --workspace
cargo fmt --all -- --check
```

## 4. 与发布门槛的差距

| 门槛                     | 状态               | 说明                                                                                                                                               |
| ------------------------ | ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| SGF 不静默丢失数据       | 部分完成           | 未知属性和变化树已保留；已支持严格 `CA` 白名单解码和无损写回拒绝，但仍缺真实历史棋谱 corpus、更多编码、格式选项、property tests 与更广泛差分覆盖。 |
| 旧格式可导入             | 未开始             | NGF/GIB/UGF 仍只由 Electron/JavaScript 实现。                                                                                                      |
| 设置和用户样式安全迁移   | 部分完成           | 已验证关键设置类型、报告未知/无效值、创建不可变备份与单次标记，并以可恢复多文件写入落盘；仍缺跨版本默认值策略、共享 fixture 与原生对话框 e2e。     |
| 常用 GTP 引擎可用        | 未开始             | 只有协议解析和基础传输，尚无监督、同步和分析流。                                                                                                   |
| UI 无 Node/Electron 特权 | 当前迁移 UI 已满足 | 新增 Tauri 组件经 bridge/store 工作；完整 UI 尚未迁移。                                                                                            |
| 插件边界可验证           | 部分完成           | manifest 与原生授权存在；WASM sandbox 和完整生命周期尚未落地。                                                                                     |
| 支持平台可构建和验证     | 部分完成           | Ubuntu CI 已验证 Rust 格式与 workspace tests；跨平台 Tauri 构建、签名、更新和平台 e2e 仍未实现。                                                   |

## 5. 当前技术风险

1. `GameDocument`
   在历史记录和快照生成中仍会复制/遍历整个树；尚未通过大型棋谱基准验证内存与导航延迟。
2. 多个互斥节点注释目前通过顺序 property
   transactions 更新。store 能保证顺序，但宿主尚未提供原子批事务，失败时存在部分应用的理论可能。
3. SGF 已支持 `CA`
   声明的 UTF-8、Shift_JIS、EUC-JP、GBK 与 Big5，并在无法严格解码或无损写回时拒绝操作；但仍缺真实历史棋谱 corpus、更多遗留编码及跨平台行为证据。旧格式导入与编码兼容性仍是用户数据风险最高的区域。
4. 设置迁移已经验证支持键和值类型、保留原文件、创建不可变备份、写入完成标记并在多文件失败时尝试恢复；但尚未以共享 fixture 锁定全部 Electron 默认值，且无原生对话框 e2e。
5. 当前文件工作流已提供宿主拥有的最近文件、单文档 crash-recovery 自动保存和基于内容指纹的外部修改检测：恢复草稿始终在 app
   config 的 `recovery.json`
   中，与原 SGF 完全分离；恢复后的无路径 dirty 文档只能 Save
   As。外部检查当前由窗口聚焦触发，尚未使用跨平台 watcher，也没有 Tauri
   e2e；仍需恢复写入节流、多文档恢复策略，以及恢复/文件对话框/最近文件/窗口关闭/外部冲突的原生 e2e。

## 6. 下一执行重点

下一阶段命名为**迭代 C′：数据保真与文件工作流**，具体范围、依赖和验收条件见
[`tauri-migration-next-steps.md`](tauri-migration-next-steps.md#迭代-c数据保真与文件工作流)。

该阶段优先证明现有可编辑棋盘不会损坏用户数据，并让用户能通过受限宿主能力完成打开、编辑和保存；它不扩展 GTP 或插件 API。
