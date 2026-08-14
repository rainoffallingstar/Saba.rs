# 交接文档（2026-08-15）

本交接文档面向接手 Sabaki 原生迁移的开发者，记录当前架构决策、已实现范围、
关键约定与下一步计划。它是 [`tauri-migration-status.md`](tauri-migration-status.md)
（历史迁移状态）、[`tauri-rearchitecture-design.md`](tauri-rearchitecture-design.md)
（架构设计）与 [`tauri-migration-next-steps.md`](tauri-migration-next-steps.md)
（路线图）的最新执行快照。

**最近迭代（2026-08-15）：** 迭代 1-5（可用性/数据保真、设置面板、真实引擎会话、
插件面板、分析流与计分）与迭代 6「跨平台与工程卫生」已完成。迭代 6：CI 矩阵扩展为
Ubuntu/macOS/Windows 三平台全绿；`main.rs`（2799 行）渲染树拆分至
`panels.rs`（976 行），shell 只留状态/动作/装配；新增 300 手职业对局与多分支教学谱
性能基线（打开/快照/导航）；`docs/packaging-notes.md` 打包调研。
`cargo test --workspace` **201 测试全绿**。

---

## 1. 架构决策（2026-08）

**全力优先发展 GPUI；永久暂停 Tauri 回退。**

- `apps/sabaki-gpui` 已从 spike 升级为**正式 GPUI 主客户端**（原 `sabaki-gpui-spike`）。
- `src-tauri`（Tauri/Preact）**冻结为行为参考**：保留在 SabakiHQ/Sabaki 上游仓库，
  **本仓库不包含** Tauri adapter 与 Electron/Node.js 源码。
- Electron 版本在 GPUI 达到公开 Beta 质量前仍是行为参考与稳定发行版。
- 所有 UI 无关逻辑收敛到 `crates/sabaki-host`，视图只通过 typed ports 与 DTO 通信；
  GPUI 客户端与上游冻结的 Tauri adapter 共享同一套 host 边界。

## 2. Workspace 结构

```
crates/domain-core      UI 无关领域核心：GameDocument / SGF / 棋盘快照 / GTP 解析 / 进程传输
crates/plugin-runtime   插件类型、manifest 校验、权限模型、JSON-RPC 帧协议、native 进程
crates/sabaki-host      UI 无关应用工作流（主战场）
apps/sabaki-gpui        GPUI 主客户端（唯一持续开发目标）
```

依赖方向：`domain-core` ← `sabaki-host` ← `apps/sabaki-gpui`；`plugin-runtime` 被
`sabaki-host` 与 GPUI 客户端消费。共享差分 fixture 位于
`crates/domain-core/tests/fixtures/differential/`。

## 3. 当前架构状态

### 3.1 `sabaki-host`：UI 无关工作流（全部带注入边界 + 单元测试）

| 模块 | 能力 | 注入边界 |
|---|---|---|
| `lib.rs` | `HostApplication`：new/open/save/save-at/事务/undo/redo/恢复/丢弃来源 | `GameFileAccess`、`HostEventSink` |
| `file_codec.rs` | **SGF 字节编解码唯一权威**：`CA` 驱动的 UTF-8/Shift_JIS/EUC-JP/GBK/Big5 严格解码与无损编码、`FileCodecError` typed error | — |
| `persistence.rs` | autosave（recovery）与 recent-files 工作流、失败回滚 | `HostPersistence` |
| `external_file.rs` | 外部 SGF 修改检测（SHA-256 基线、clean 自动重载、dirty 冲突） | `ExternalFileReader` |
| `close_flow.rs` | dirty 关闭决策 | — |
| `settings.rs` | **键表唯一权威**（完整 Electron 键表）、类型校验、`SettingsStore`、load/persist 工作流 | `SettingsPersistence` |
| `engine_workflow.rs` | `engines.list` 严格校验、`EngineRecord`、`EngineStore`（绑定设置键） | — |
| `engine_session.rs` | GTP 会话生命周期：握手/启动命令/boardsize/play/genmove/stop | `GtpTransport` |
| `plugin_workflow.rs` | 插件安装扫描、`PluginStore`（扫描/显式两种恢复）、权限/授权、持久化 | `PluginPersistence` |

**关键原则**：host 不碰文件系统/进程——所有 IO 经 trait 注入，测试用内存实现，
生产用各端文件系统实现。键表（`setting_kind`）、校验（`validate_setting_value`）、
引擎列表校验均以 host 为唯一权威，Tauri/GPUI 不再维护平行副本。

### 3.2 `apps/sabaki-gpui`：GPUI 主客户端

基于 GPUI 0.2.2（`macos-blade` 后端），已具备：

- **Shell**：窗口、菜单、快捷键、启动文件、对话框 port（`DialogService`，
  生产实现为 `RfdDialogService`——rfd 原生打开/保存面板，另存为自动补 `.sgf`
  扩展名并建议当前来源文件名）。
- **8 个功能组件簇**：导航、变化树、标记工具、节点检查器、文件工作流、设置、
  引擎控制台、插件面板——全部经 host typed API 驱动。
- **引擎控制台与真实会话**：`EngineStore`（绑定 `engines.list`）加载引擎列表；
  引擎管理（connect/disconnect、remove、`Name | path | args | commands` 规格输入
  添加、经设置持久化）；连接后 `EngineSession::start(ProcessGtpTransport, ...)`
  握手/能力探测/启动命令/boardsize/clear_board 并重放当前局面；控制台命令经
  `send_command` 转发；「engine move」按钮 genmove 当前手方并落子；
  人类落子经 `play` 同步引擎；换局/换棋盘/恢复时断开。无引擎时回退 `MockGtpEngine`。
  GTP 坐标转换按协议跳过 I 列（`J3` = 第 8 列）。
- **设置**：主题 Classic/Dark/Mist + 键表驱动设置表单（`settings_form.rs`：
  高频键子集的 Boolean toggle / Number 输入，host 校验、持久化失败回滚），
  设置读写全部经 `SettingsStore` + `NativeSettingsPersistence`。
- **窗口状态**：启动恢复 `window.width/height` **与 `window.maximized`**
  （`WindowBounds::Maximized` + restore bounds），关闭时经
  `on_window_should_close` 落盘。
- **dirty 关闭确认**：关闭时经 host `decide_close_request` 决策，dirty 文档弹
  原生 Save/Discard/Cancel 确认；保存失败或取消则保持窗口打开。
- **文件编码**：`NativeGameFileAccess` 复用 `sabaki_host::file_codec`，
  支持全部 `CA` 声明的编码（UTF-8/Shift_JIS/EUC-JP/GBK/Big5）与原子写入，
  无损性拒绝策略与冻结的 Tauri 参考一致。
- **外部文件变更检测**：`NativeExternalFileReader`（解码内容指纹）+
  host `ExternalFileStore`；打开/保存后建立基线，窗口激活后（帧调度 + 1s
  节流）检查：clean 变更自动重载并重建基线，dirty 冲突显示
  reload external / keep local（Save As）操作。
- **插件面板**：启动时 `PluginStore::restore` 扫描 `<config>/plugins` 安装根目录；
  行内显示缺失权限与 native 运行时状态；「grant & enable」授予 manifest 权限并启用，
  「authorize native」经 rfd 二次确认后授权+授予+启用；启用插件渲染
  `contributes.commands` 为可点击分发按钮（declarative 暂记录状态栏）；
  每次变更 persist，重启恢复启用态；空安装根显示提示。
- **真实配置目录**（`$HOME/.config/sabaki-gpui` 或 `SABAKI_CONFIG_DIR`）：

| 数据 | 文件 | 实现 |
|---|---|---|
| 设置 | `settings.json` + `styles.css` | `NativeSettingsPersistence` |
| crash-recovery | `recovery.json` | `NativeHostPersistence` |
| recent-files | `recent-files.json` | `NativeHostPersistence` |
| 插件注册表 | `plugins.json` | `NativePluginPersistence` |

文件布局与冻结的 Tauri 参考实现一致，同一配置目录可在两端互换。

### 3.3 `src-tauri`（冻结，本仓库不包含）

保留在 SabakiHQ/Sabaki 上游仓库作为行为参考：完整的文件服务（多编码）、
设置迁移、recent/autosave/外部文件、插件命令等 31 个测试全部通过。
不再新增开发；本仓库不包含其源码与测试。

## 4. 关键约定

- **事务**：所有编辑经 `GameTransaction`（camelCase DTO），revision 保护。
- **键表**：`sabaki_host::settings::setting_kind` 是唯一权威；`engines.list` 为对象数组
  （`EngineRecord`），其余键按 Boolean/Number/String/NullableString/StringArray 校验。
- **错误**：host 返回 `HostError`/`SettingValidationError` 等 typed error；
  上游 Tauri 侧映射为 `CommandErrorDto`（code/message/details），本仓库不包含该映射。
- **测试**：每层各自测试（`cargo test --workspace` 全绿），注入边界用内存替身；
  真实文件系统测试写 `std::env::temp_dir()` 下的唯一目录并清理。

## 5. 验证状态

| 目标 | 数量 |
|---|---|
| `domain-core` | 15 单元 + 8 差分 fixture（含计分覆盖事务） |
| `sabaki-host` | 75 单元 + 5 workflow 集成 + **2 真实子进程冒烟**（fake-gtp-engine.py）+ 7 分析解析 |
| `sabaki-gpui` | 103 测试（含真实文件系统往返、外部文件检测、关闭决策、设置表单、引擎管理、插件全流程、分析选择、大棋谱基准、棋盘渲染几何与 setup/计分事务） |

构建/测试命令：

```bash
cargo test --workspace        # 全部测试（当前 201 个全绿）
cargo test -p sabaki-host     # host 工作流
cargo test -p sabaki-gpui     # GPUI 客户端
cargo run -p sabaki-gpui      # 启动 GPUI 客户端（可传 SGF 路径参数）
SABAKI_CONFIG_DIR=/tmp/sg cargo run -p sabaki-gpui   # 指定配置目录
```

## 6. 下一步计划（GPUI 优先路线）

**迭代 1（可用性/数据保真）已完成：** 基线提交、编码策略共享（file_codec 上移
host）、rfd 原生对话框、dirty 关闭确认、window.maximized、外部文件变更检测。
**迭代 2（设置面板补全）已完成：** 键表驱动设置表单（高频键子集）。
**迭代 3（真实引擎会话）已完成：** `EngineSession` 能力探测 + `send_command` +
`MissingVersion`；引擎管理 UI；`ProcessGtpTransport` 接入控制台与对局；
GTP 坐标 I 列规则；`examples/fake-gtp-engine.py` 真实子进程冒烟测试。
**迭代 4（插件面板补全）已完成：** 安装根扫描 + 权限授予 + native 授权 + 命令分发。
**迭代 5（分析流与计分）已完成：** domain `applyScoringOverride`（-1/0/1、undo/redo）；
host `analysis` 模块（lz-analyze/kata-analyze 解析）+ `EngineSession::analyze`/
`stop_analysis`；GPUI analyze 按钮、top-3 候选、胜率条、棋盘最佳着手标记。

**迭代 6（跨平台与工程卫生）已完成：** CI 矩阵（Ubuntu/macOS/Windows）全绿、
shell 拆分（`panels.rs`）、大棋谱性能基线、打包调研文档。

**迭代 7（棋盘渲染与编辑补全）已完成：** 线/箭头、坐标/手数/last-move、
setup stones、计分模式 UI。

按优先级排序的后续候选迭代：

1. **分析流式更新**：KataGo `kata-analyze` 实时流（非阻塞读取 + 节流事件）、
   `engines.analyze_commands` 配置接入。
2. **发布准备第二阶段**：release 构建 CI、macOS bundle/dmg、Linux AppImage、
   Windows installer、签名/公证（见 `docs/packaging-notes.md`）。
3. **路线图远期项**：WASM runtime + capability imports、插件私有存储与
   native 进程监督（超时/重启/日志）、NGF/GIB/UGF 导入、主题包安装、
   SGF property tests、Beta 门槛验证。

## 7. 技术债 / 已知限制

- 引擎真实进程路径已有真实子进程冒烟测试（fake-gtp-engine.py），但尚未用
  KataGo/GNU Go 实物引擎做过手工验证（需用户配置引擎 + 模型）；KataGo
  `kata-analyze` 的实时流式搜索更新（非阻塞读取 + 节流事件）属后续监督阶段。
- `MemorySettingsPersistence`/`MemoryHostPersistence`/`MemoryPluginPersistence` 保留供测试，
  生产路径已全部走 Native 实现。
- `main.rs` 已拆分（`panels.rs`），但 `ShellApp` 状态字段与事件处理器仍集中在
  main.rs；后续按需继续细化。
- gpui 0.2.2 无公开窗口激活事件，外部文件检查以帧调度 + `is_active` + 节流实现
  （窗口激活会触发 refresh 帧）；无原生对话框 API，经 rfd 实现。
- 计分覆盖已实现（`ApplyScoringOverride` + `GameSnapshot.score_overrides`），
  但尚无计分模式 UI 与死子判定算法（`score.estimator_iterations` 键未用）。
- 分析胜率条将 winrate 直接按黑方显示（未做手番换算）；setup 工具与计分模式
  已可用但缺少单独的 SGF 往返集成测试（依赖差分 fixture 的后续扩充）。
- Tauri 冻结，其未完成项（theme/plugin 错误 DTO、原生 e2e）不再安排。
