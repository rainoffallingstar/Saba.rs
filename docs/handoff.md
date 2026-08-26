# 交接文档（2026-08-16）

本交接文档面向接手 Sabaki 原生迁移的开发者，记录当前架构决策、已实现范围、
关键约定与下一步计划。它是 [`tauri-migration-status.md`](tauri-migration-status.md)
（历史迁移状态）、[`tauri-rearchitecture-design.md`](tauri-rearchitecture-design.md)
（架构设计）与 [`tauri-migration-next-steps.md`](tauri-migration-next-steps.md)
（路线图）的最新执行快照。发布候选的真实状态、外部依赖与下一步证据见
[`release-readiness.md`](release-readiness.md)。

**最近迭代（2026-08-21）：** 插件不可用根因修复（命令 ID 命名空间校验）、插件扫描容错、点目模式防卡死、ZIP 选择器过滤修复。
依据用户最新实测反馈完成落地：
1. **插件不可用根因修复（命令 ID 命名空间校验）**（`examples/plugins/*/ryusei-plugin.json` + `main.rs`）：
   - 定位到 `katago-setup-hub` 与 `fox-kifu-sync` 插件的命令 ID 未以插件 ID 为命名空间前缀（如 `org.ryusei.katago.setup` 而非 `org.ryusei.katago-setup-hub.setup`），触发 `PluginManifest::validate` 的 `command identifiers must be namespaced by the plugin id` 校验失败，导致插件被整体拒绝加载；
   - 已修正全部命令 ID 与面板按钮 ID 为正确的命名空间前缀，并同步更新 `on_plugin_command` 分发逻辑。
2. **插件扫描容错（单插件损坏不再拖垮全部）**（`crates/ryusei-host/src/plugin_workflow.rs`）：
   - `scan_plugin_installations` 由「任一插件无效即整体失败」改为「跳过无效插件并继续扫描」，单个损坏插件不再导致整个插件库为空。
3. **点目（Scoring）模式防卡死与一键退出**（`main.rs` + `panels.rs`）：
   - 在点目/估目模式下，底部状态栏高亮展示 `[ 🏁 退出点目 (返回落子) ]` 显式胶囊，点击或按 `Cmd-1` 立即退出点目并重置工具为落子工具，恢复正常下棋落子。
4. **ZIP 文件选择对话框类型过滤修复**（`dialog_service.rs` + `main.rs`）：
   - `RfdDialogService` 新增专门的 `pick_open_zip_path`，配置 `Plugin Archive (*.zip)` 过滤器，允许在 macOS 访达中直接选中并导入 `.zip` 插件包。
5. **棋盘落子即时响应修复**（`apps/ryusei-gpui/src/main.rs`）：
   - 落子触发时机移至 `on_board_vertex_mouse_down`，鼠标按下瞬间立即完成逻辑落子、棋盘图元刷新与音效播发。
6. **插件全量打包为离线 ZIP 归档**（`examples/plugins/*.zip`）：
   - 已生成标准独立的 `.zip` 安装包（`katago-setup-hub.zip`、`fox-kifu-sync.zip`、`position-checker.zip`、`sgf-exporter.zip`）。
7. **设置抽屉新增插件管理中心**（`panels.rs` + `main.rs`）：
   - 在 Preferences 设置抽屉中新增 **`PLUGINS & EXTENSIONS (插件管理)`** 专区，提供一键开启/关闭与 ZIP 导入。
已通过 fmt、140 个 GPUI 单元测试、完整 workspace 测试与 release build 验证。

**最近迭代（2026-08-17）：** 已修复首启布局；macOS fullscreen-exit renderer 修复正在重新验收。
此前缺失 `view.show_leftsidebar`、`view.show_graph`、`view.show_comments` 设置会被错误解释为
false，因此新配置没有两侧栏；现在缺失代表首次启动，默认呈现 Engines 和 Panels，而持久化 false
仍保留用户选择。多行开发 status/benchmark 区已收敛为单行状态栏，不再压缩棋盘高度；
`fresh_settings_render_both_sidebars_and_a_compact_status_bar` 锁定两个症状。
macOS 26.6.1 / Apple M4 的人工 crash report 定位到 GPUI 0.2.2 `macos-blade`：AppKit 退出
fullscreen 设置 style mask → frame resize → `BladeRenderer::update_drawable_size` →
`EXC_BAD_ACCESS`。先前只避免 GPUI 恢复 transparent titlebar 的 patch 不充分：后续报告确认
AppKit 的 `_NSExitFullScreenTransitionController` 自己仍会更新 style mask 并触发 resize。
当前 vendor GPUI patch 在 `windowWillExitFullScreen:` 到 `windowDidExitFullScreen:` 之间延迟
所有 Blade drawable resize（frame 与 backing-scale 两入口），并在 exit 完成后只按最终尺寸刷新一次。
它不改变 Sabaki transaction、GTP、音效或普通 resize；详见
`docs/gpui-macos-fullscreen-workaround.md`。已通过 fmt、140 个 GPUI 测试、完整
workspace/doc-tests 和 release build；新 release binary 的重复原生 fullscreen enter/exit 人工
复测未再崩溃。该检查仍保留在 `docs/release-qa.md` 供每一个发布候选执行。

**macOS 文件对话框修复（2026-08-17）：** 另一份 crash report 显示 File → Open 的 `rfd`
同步 NSOpenPanel 在嵌套 AppKit loop 内收到键盘输入源通知；GPUI 0.2.2 随即在已有 menu-action
借用之上 `App::borrow_mut()`，造成 `RefCell already borrowed` / SIGABRT。vendor GPUI patch
改用 `try_borrow_mut()`，重入通知只跳过当次 layout refresh 而非 abort。新增可注入的 test-platform
notification seam，以及两个回归：reentrant action 不 abort、idle observer 仍收到更新。旧实现可使前者
稳定 red。修复后已执行完整 workspace/release 验证，并在 macOS release binary 中人工确认 Open 和
Save As 在输入源切换下不再崩溃、实际打开/保存均正常；细节见
`docs/gpui-macos-file-dialog-workaround.md`。签名与公证继续不执行。

---

## 1. 架构决策（2026-08）

**全力优先发展 GPUI；永久暂停 Tauri 回退。**

- `apps/ryusei-gpui` 已从 spike 升级为**正式 GPUI 主客户端**（原 `ryusei-gpui-spike`）。
- `src-tauri`（Tauri/Preact）与 Electron/Node.js 实现**冻结为行为参考**：本地源码和
  原 Git 历史保存在被主线忽略的 `refer-repo/`，不参与 Ryusei 构建、CI 与发布。
- Electron 版本在 GPUI 达到公开 Beta 质量前仍是行为参考与稳定发行版。
- 所有 UI 无关逻辑收敛到 `crates/ryusei-host`，视图只通过 typed ports 与 DTO 通信；
  GPUI 客户端与上游冻结的 Tauri adapter 共享同一套 host 边界。

## 2. Workspace 结构

```
crates/domain-core      UI 无关领域核心：GameDocument / SGF / 棋盘快照 / GTP 解析 / 进程传输
crates/plugin-runtime   插件类型、manifest 校验、权限模型、JSON-RPC 帧协议、native 进程
crates/ryusei-host      UI 无关应用工作流（主战场）
apps/ryusei-gpui        GPUI 主客户端（唯一持续开发目标）
```

依赖方向：`domain-core` ← `ryusei-host` ← `apps/ryusei-gpui`；`plugin-runtime` 被
`ryusei-host` 与 GPUI 客户端消费。共享差分 fixture 位于
`crates/domain-core/tests/fixtures/differential/`。

## 3. 当前架构状态

### 3.1 `ryusei-host`：UI 无关工作流（全部带注入边界 + 单元测试）

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

**关键原则**：领域和主要 Host 工作流的 IO 经 trait 注入，测试使用内存 Adapter，
生产使用原生 Adapter。当前 `katago_setup`/`fox_kifu` 中仍存在直接文件系统、系统命令和
`curl` 副作用，是审查确认的 seam 回退，按 `release-remediation-plan.md` P2.3 修复。
键表（`setting_kind`）、校验（`validate_setting_value`）和引擎列表校验仍以 host 为唯一权威。

### 3.2 `apps/ryusei-gpui`：GPUI 主客户端

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
- **文件编码**：`NativeGameFileAccess` 复用 `ryusei_host::file_codec`，
  支持全部 `CA` 声明的编码（UTF-8/Shift_JIS/EUC-JP/GBK/Big5）与原子写入，
  无损性拒绝策略与冻结的 Tauri 参考一致。
- **外部文件变更检测**：`NativeExternalFileReader`（解码内容指纹）+
  host `ExternalFileStore`；打开/保存后建立基线，窗口激活后（帧调度 + 1s
  节流）检查：clean 变更自动重载并重建基线，dirty 冲突显示
  reload external / keep local（Save As）操作。
- **插件面板**：启动时 `PluginStore::restore` 扫描 `<config>/plugins` 安装根目录；
  行内显示缺失权限与 native 运行时状态；「grant & enable」授予 manifest 权限并启用，
  「authorize native」经 rfd 二次确认后授权+授予+启用；启用插件渲染
  `contributes.commands` 为可点击分发按钮与 `contributes.panels` 声明式
  面板（迭代 27 起读取真实 manifest，不再使用硬编码 demo）；
  每次变更 persist，重启恢复启用态；空安装根显示提示。
- **真实配置目录**（`$HOME/.config/ryusei-gpui` 或 `SABAKI_CONFIG_DIR`）：

| 数据 | 文件 | 实现 |
|---|---|---|
| 设置 | `settings.json` + `styles.css` | `NativeSettingsPersistence` |
| crash-recovery | `recovery.json` | `NativeHostPersistence` |
| recent-files | `recent-files.json` | `NativeHostPersistence` |
| 插件注册表 | `plugins.json` | `NativePluginPersistence` |

文件布局与冻结的 Tauri 参考实现一致，同一配置目录可在两端互换。

### 3.3 `refer-repo/`（冻结、主线忽略）

旧 Electron/Tauri Sabaki 的源码、测试及独立 Git 历史保存在 `refer-repo/`，用于多编码
文件服务、设置迁移、recent/autosave、外部文件、插件与 UI 行为对照。该目录不再新增
主线功能，也不参与当前 Cargo workspace、CI、Tag 或 Release。

## 4. 关键约定

- **事务**：所有编辑经 `GameTransaction`（camelCase DTO），revision 保护。
- **键表**：`ryusei_host::settings::setting_kind` 是唯一权威；`engines.list` 为对象数组
  （`EngineRecord`），其余键按 Boolean/Number/String/NullableString/StringArray 校验。
- **错误**：host 返回 `HostError`/`SettingValidationError` 等 typed error；
  上游 Tauri 侧映射为 `CommandErrorDto`（code/message/details），本仓库不包含该映射。
- **测试**：每层各自测试（`cargo test --workspace` 全绿），注入边界用内存替身；
  真实文件系统测试写 `std::env::temp_dir()` 下的唯一目录并清理。

## 5. 验证状态

| 目标 | 数量 |
|---|---|
| `domain-core` | 27 单元（含 9 计分器、标准/Tygem 让子摆位）+ 8 差分 fixture（含计分覆盖事务、流式进程 2 冒烟）+ 5 legacy fixture 导入集成 + 5 SGF proptest |
| `plugin-runtime` | 26 测试：8 存储 + 5 监督进程（python3 冒烟）+ 11 wasm 沙箱（含 capability imports/事务提议）+ 2 帧/校验 + 2 声明式面板 manifest 校验 |
| `ryusei-host` | 102 单元（含 3 监督进程冒烟含真实 Go 插件 e2e、2 流式会话复用、5 wasm 工作流含事务提议、8 主题包、4 styles 迁移分析、干净新局根属性）+ 5 workflow 集成 + **2 真实子进程冒烟**（fake-gtp-engine.py）+ 9 分析解析/重放 + **2 legacy open 分发** |
| `ryusei-gpui` | 115 测试（含 4 headless 逻辑冒烟、1 完整窗口渲染帧冒烟（三栏 bounds、落子、Pass、分栏拖拽与持久化）、4 分栏纯函数、3 模式栏纯函数、新局默认值、明暗 UiPalette、2 计分摘要;theme tokens 校验测试随类型上移 host）（含真实文件系统往返、外部文件检测、关闭决策、设置表单、引擎管理、插件全流程、流式分析合并/命令选择、大棋谱基准、棋盘渲染几何与 setup/计分事务） |

构建/测试命令：

```bash
cargo test --workspace        # 全部测试（当前 304 个全绿）
cargo test -p ryusei-host     # host 工作流
cargo test -p ryusei-gpui     # GPUI 客户端
cargo run -p ryusei-gpui      # 启动 GPUI 客户端（可传 SGF 路径参数）
SABAKI_CONFIG_DIR=/tmp/sg cargo run -p ryusei-gpui   # 指定配置目录
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
**迭代 8（分析流式更新）已完成：** kata-analyze/lz-analyze 实时流
（`AnalysisStream` 后台线程 + 120ms 合并刷新）、`engines.analyze_commands` 配置、
stop analysis、generation 取消旧任务。
**迭代 9（发布准备第二阶段）已完成：** release 构建 CI 三平台产物上传
（macOS .app+dmg / Linux tar.gz / Windows zip），bundle 脚本可用。
**迭代 10（数据保真加固 + 旧格式导入）已完成：** NGF/GIB/UGF 导入器
（`domain-core::legacy`，语义镜像上游 `src/modules/fileformats/*.js`，含 tygem
让子落位顺序、UGF 坐标换算、`[` 转义、`parseFloat` 前缀语义）；`file_codec`
新增 `decode_legacy_bytes`（UTF-8→Shift_JIS→EUC-JP→GBK→Big5 严格尝试，字节
重叠歧义按候选顺序确定性决策）；host `open()` 与 GPUI 原生文件访问按扩展名
分发导入（导入结果归一化为 UTF-8 SGF），SGF 路径保留 `CA` 检测出的源编码
（修复此前硬编码 Utf8 的问题）；真实 fixture 集成测试（even/handicap2.ngf、
utf8/euc-kr.gib、amateur.ugf、gb2312.ngf）+ host `open` 分发测试；
**SGF property tests（proptest）**：序列化幂等、move 序列/棋盘/根属性往返
一致、任意合法对局不 panic（`domain-core/tests/sgf_properties.rs`）。
**迭代 11（插件深水区第一波）已完成：** `plugin-runtime::storage` 插件私有
存储（插件 ID 命名空间键值 JSON、原子写、key/路径校验、单值 1MiB 与 4096 键
限制）；`SupervisedNativePluginProcess`（stdout 读线程按请求 ID 分发响应、
命令超时、崩溃检测 `ProcessExited`、stderr 环形日志 200 行×512 字符、重启
上限 3 次，真实 python3 子进程测试）；host `PluginSupervisor`（请求/重启/
停止/轮询、崩溃诊断、超过 `AUTO_DISABLE_AFTER_CRASHES` 自动标记禁用，对应
设计 §10.4「每次崩溃记录诊断并自动禁用，避免无限重启」）；gpui 插件面板
显示进程状态（running/crashed/auto-disabled）与最近 stderr 日志，启用原生
插件时自动启动监督进程、禁用时停止。

**迭代 12（WASM runtime）已完成：** `plugin-runtime::wasm` 沙箱运行时
（wasmi 1.1，无 host import，符合 §10.3）：导出 `memory` + `invoke` 的 ABI、
每次调用 fuel 上限 1M、内存上限 32 页、payload 1MiB、递归深度 128；模块
编译校验（缺 invoke/缺 memory/声明内存超限均拒绝）、无效 JSON 响应拒绝、
死循环 fuel trap，7 个 WAT 内嵌测试。host `plugin_wasm` 工作流：加载
`.wasm` entrypoint（runtime/enable/扩展名校验）、`invoke_wasm_command`
（JSON-RPC 形状 DTO）、错误映射到共享 `PluginError` 词汇。GPUI 命令按钮
对 wasm 插件真实调用并显示结果（声明式面板在迭代 27 改为 manifest
`contributes.panels` 真实渲染）。

**迭代 13（主题包安装）已完成：** `ryusei-host::theme_workflow`（设计 §8.2）：
`ThemeManifest`（theme.json：schemaVersion/id/name/version/assets 允许列表）、
`ThemeTokens` 从 gpui 上移至 host（schema 版本 + hex 颜色校验）、`InstalledTheme`
加载（manifest+tokens+资源存在性与 10MiB 大小上限）、`scan_theme_root`（扫描
已安装主题 + **旧 `.asar` 主题只报告迁移说明、不执行不解包**）、
`install_theme`（校验后复制）/`uninstall_theme`；路径穿越/非法扩展名/坏 token
均拒绝。GPUI 设置面板列出已安装主题（点击应用 `theme:<id>` 并持久化）、
`.asar` 主题显示红色迁移提示；`crate::theme` 改为 host re-export 消除重复。

**迭代 14（WASM capability imports）已完成：** `WasmCapabilities`（默认无
import；授权时才定义 `ryusei.game_snapshot`，未授权 import 在 link 阶段即
失败，落实 §10.3「最小 capability import」）；host
`wasm_capabilities_for`（按 granted `GameRead` 决定是否注入快照）与
`invoke_wasm_command` 增加快照参数；GPUI 命令分发把当前 `GameSnapshot`
序列化注入插件。WAT 测试：授权可调用并转发结果、未授权 link 失败。

**迭代 15（Beta 门槛加固）已完成：** `ryusei-host::legacy_styles`
`analyze_legacy_styles`（设计 §8.1:不承诺 styles.css 运行时兼容,可表达为
theme-token 的颜色规则列出、其余规则计为忽略;容错解析注释/缺分号/短 hex/
rgb()）;GPUI 启动时对非空 user styles.css 生成迁移报告并显示在状态栏;
新增 `docs/beta-gate.md` §11.3 十项逐项核对表:九项满足/部分满足,唯一
缺口为 #10 原生 screenshot/headless GPU CI(gpui 0.2.2 能力限制,附短期/
中期/里程碑建议)。

**迭代 16（WASM 事务写入）已完成：** `WasmCapabilities.game_write`
（授权 `GameWrite` 时暴露 `ryusei.game_submit_transaction` import,未授权
link 失败）;插件提交的事务 JSON 被**收集为 proposal**(不直接改游戏),
invoke 后由宿主 `take_pending_transactions` 取出,`invoke_wasm_command`
返回 `WasmInvocationResult { response, proposed_transactions }`;GPUI
命令分发对每个 proposal 反序列化为 `GameTransaction` 并经
`HostApplication::apply_transaction` 验证应用(非法落子/劫/占用由宿主拒绝),
状态栏报告 applied/rejected 计数。WAT 测试:授权收集、未授权 link 失败。

**迭代 17（主题包安装入口 UI）已完成：** GPUI 设置面板新增
「install theme from folder…」按钮（原生目录选择 → `install_theme` 校验后
复制 → 刷新列表）与每个已安装主题的「uninstall」按钮;应用/卸载后状态栏
反馈。至此设计 §8.2 主题包流程（发现/校验/安装/卸载/应用/资源路径控制/
`.asar` 迁移提示）闭环。

**迭代 25（headless 冒烟测试）已完成：** 用 gpui `test-support` 的
`TestAppContext` 在无窗口/无 GPU 环境构造完整 `ShellApp` 实体,headless
执行核心交互:开局/落子、计分模式覆盖、主题 token 应用、分析命令读取
（4 测试,`apps/ryusei-gpui` dev-deps 增加 gpui test-support + rand）。
这是 Beta #10 的务实部分:渲染截图仍受 gpui 0.2.2 能力限制（见
`docs/beta-gate.md`）,但应用逻辑层已纳入 headless CI。

**迭代 26（前端审查与 Beta #10 渲染帧冒烟）已完成：** 修复实测前端阻断项：
（1）棋盘点击无法落子——原 `render_goban_area` 把 `on_mouse_down` 挂在仅含
absolute 子元素、自身为 0×0 的 div 上，命中框为空；且事件坐标先减窗口偏移再
反推 vertex，叠加渲染根又额外偏移半 margin，命中即偏。现改为
`render_goban_click_layer` 为每个交点生成显式 `spacing×spacing` 透明 hitbox，
closure 直接携带 `Vertex` 调用 `on_board_vertex_clicked`，不再依赖窗口全局坐标。
（2）棋盘渲染偏移——`render_goban` 木底 absolute 半 margin 内又嵌套一个 relative
全尺寸画布，导致线条/棋子/标记整体再偏移半 margin；现木底为 absolute 子层，
线条/棋子/标记直接位于全尺寸 relative 根中。
（3）页面排版混乱——原窗口所有区块均为绝对定位硬编码坐标，在默认窗口下插件/
变化树/引擎/节点检查器/设置互相重叠或超出窗口；现改为 flex 列布局（header /
左侧 468px 棋盘列（棋盘、工具栏、recovery/外部冲突按钮）/ 右侧可滚动
sidebar（插件、变化树、引擎、节点检查器、设置五个卡片）/ status bar），
面板之间不再重叠，窄窗口纵向可滚动。默认窗口尺寸 1060×640 → 1240×800。
（4）回归测试——新增 `frontend_smoke` 完整窗口测试：gpui test platform 绘制
`ShellApp`，经 debug selector 断言 goban 420×420、木底 392×392 且距 goban
原点 14px、board-column 与 sidebar 不重叠、五个面板均在 sidebar 内且垂直顺序
堆叠，并模拟鼠标点击第 17 个交点后断言黑子落于 (16,16)。这使 Beta #10 从
“仅应用逻辑 headless”前进到“完整窗口 layout/paint + 输入 dispatch”；
原生 screenshot 仍待上游能力。

**迭代 27（GPUI 前端接线与声明式插件面板）已完成：**
（1）设置面板接线——`game.default_board_size/komi/handicap` 现在驱动
New Game 与启动新局；host 新增 `create_new_with_properties`，在构造阶段
写入 `KM`/`HA`/标准 `AB` 让子，保持初始文档 clean；domain 新增标准让子
摆位函数（与 legacy Tygem 摆位并存）；`view.show_graph` 控制变化树面板，
`view.show_comments` 控制注释编辑区，`board.show_analysis` 控制分析标记，
`gtp.console_log_enabled` 控制控制台记录与展示；`sound.enable` 在音频
子系统落地前从设置面板移除。
（2）Pass 按钮——工具栏新增 Pass，经 `Pass` 事务同步引擎并写 recovery；
渲染帧测试现在同时模拟棋盘落子与 Pass。
（3）明暗 UI 调色板——`theme.rs` 新增 `UiPalette`，从 theme background
亮度推导文本/面板/边框/按钮/危险/成功等 shell 颜色；GPUI 面板、按钮、
输入框、状态栏、变化树与棋盘坐标标签全部改用调色板，修复 Dark/Mist
下深色文字不可读问题；`render_variation_tree` 节点色也随调色板切换。
（4）声明式插件面板真实化——`PanelWidget`/`PluginPanelContribution`
上移至 `plugin-runtime`，manifest `contributes.panels` 可携带面板贡献；
`PluginPanelEntry` 暴露 `panels` 与 `ui_panel_granted`；GPUI 插件面板
移除硬编码 Opening Trainer demo，改为渲染已启用且已授权 `uiPanel` 的
真实 manifest 面板，按钮经 `on_plugin_command` 分发。
（5）窗口最小尺寸 960×640，避免固定棋盘列在窗口过小时裁切。



**迭代 28（M0 三栏布局基座）已完成：**
（1）新增 `apps/ryusei-gpui/src/layout.rs`：`SplitPane`、分栏拖拽尺寸
纯函数、min/max clamp、设置回退（`view.leftsidebar_width=250` /
`view.sidebar_width=200`）、右栏显隐推导（`show_graph || show_comments`）。
（2）`ShellApp` 新增 `left_sidebar_width` / `right_sidebar_width` 缓存与
`SplitDrag` 状态机：分栏 handle `on_mouse_down` 开始拖拽；拖拽期间渲染
全窗口透明 overlay 接收 move/up，避免鼠标移出 handle 后丢事件；松手后
写入并持久化 `view.leftsidebar_width` / `view.sidebar_width`。
（3）渲染树改为三栏：左栏承载 engine panel（GTP 控制台），中央承载
goban/工具栏/recovery/外部冲突，右栏承载 plugins、variation tree（受
`view.show_graph` 控制）、node inspector、settings；左右分栏 handle 宽
5px 可拖拽，工具栏新增 Engines / Panels 显隐按钮。
（4）默认显隐对齐原版：左栏 `view.show_leftsidebar=false`；右栏按
`show_graph || show_comments`（默认均 false），Panels 按钮同时翻转两键
保证按钮语义正确。
（5）测试：`layout.rs` 4 个纯函数测试；`frontend_smoke` 扩展到三栏
bounds、左右初始宽度、引擎面板位于左栏、四个右栏面板顺序堆叠，并模拟
拖拽左分栏 +60px 后断言宽度 310px 与设置持久化。


**迭代 29（M1 中央模式栏第一半）已完成：**
（1）新增 `mode_bar.rs`：`GameMode`（domain 已有）驱动 Play/Edit/
Scoring/Estimator 四个模式按钮；PlayBar 显示 SGF `PB/PW/BR/WR` 棋手
名称/段位与当前手方；Pass/Resign 入口；EditBar 复用 markup toolbar；
Scoring/Estimator 显示操作提示与计分摘要。
（2）`ShellApp.scoring_mode` 替换为 `mode: GameMode`；棋盘点击按模式
分发（Scoring/Estimator 循环死活标记）；选择非 Play 工具自动进入 Edit；
设置面板 scoring toggle 改为模式切换。
（3）棋盘显示补齐第一半：`view.coordinates_type` 支持 A1（SGF 字母 +
底部起算行号）与 1-1；`view.show_next_moves` 渲染子着 ghost stone；
`view.show_siblings` 渲染兄弟变化 ghost stone；`view.show_move_colorization`
在子着旁显示 BM/DO 等注释标签；设置面板新增对应键。
（4）测试：`mode_bar.rs` 3 个纯函数测试；现有 115 GPUI 测试全绿。
剩余 M1 第二半：线/箭头拖拽绘制、Find/Guess/Autoplay 模式与
`view.move_numbers_type`（variation/hotspot）。

**迭代 24（Flatpak 打包）已完成：** `flatpak/dev.ryusei.app.yml`
（Freedesktop Platform/Sdk 24.08 + rust-stable extension;finish-args 仅
显示 socket/DRI/配置目录;构建沙箱经 `build-args: --share=network` 允许
cargo 联网）;release.yml 新增 flatpak job（实测产出
`ryusei-linux-x86_64.flatpak`）,publish 等待其完成。期间修复:Flatpak
应用 ID 段不能含连字符（`dev.ryusei.app`）、Windows NSIS 安装改回
chocolatey + `--attempts=5` 重试（社区源 503 瞬时故障,sourceforge 直链
在本环境返回 HTML）。发布流水线四平台产物齐备。
**迭代 23（分析命令参数透传）已完成：** `analysis_command_from_settings`
返回 (命令名, 参数) 元组——`engines.analyze_commands` 条目可携带参数
（`"kata-analyze -visits 100"`）:已连接会话经 `stream_analyze` 逐参数
透传,回退进程路径拼接完整命令行发送;`parse_stream_line` 仍按命令名
判断 JSON/文本格式。测试覆盖无参/带参/空数组。
**迭代 22（胜率手番换算）已完成：** `best_analysis_winrate` 接受
`next_player` 参数——白方行棋时引擎胜率换算为黑方视角（1 - winrate），
胜率条始终表示黑 vs 白;测试覆盖黑/白视角与空集。
**迭代 21（引擎会话复用）已完成：** `GtpProcessSupervisor` 改为
channel+读线程模型（同步 `send` 与流式共享同一 reader,与 AnalysisStream
一致）;`GtpTransport` trait 增加 `send_streaming`/`recv_line_timeout`
（默认 `UnsupportedStreaming`,不支持流式的 transport 显式报告）;
`EngineSession::stream_analyze`/`recv_analysis_line` 在已连接会话上执行
流式分析（**不新起进程**）;GPUI 流式分析优先复用已连接会话
（boardsize/clear_board/重放 → kata/lz-analyze → 读循环 → 会话归还
shell）,`UnsupportedStreaming` 或未连接时回退独立 AnalysisStream 进程。
**迭代 20（计分器与计分摘要）已完成：** `domain-core::scoring` 区域计分
（Chinese rules,area scoring）:链检测（连通+气）、无气链死子启发
（`mark_surrounded_chains`,边界链保活）、空区域归属（双色边界区域计 seki
均不计）、用户 `score_overrides` 按链覆盖死活（rescue/kill）、捕获计入
对方、贴目与胜负/差距;GPUI 计分模式开启时面板实时显示计分摘要
（`markup::scoring_summary`:目/子/捕获/贴目/胜者与差距）。9 个 domain
测试 + 2 个 summary 测试;`score.estimator_iterations`(Monte Carlo)留待
后续。
**迭代 19（native 插件命令调用链）已完成：** GPUI `on_plugin_command`
新增 Native 分支——经 `PluginSupervisor::request` 走 JSON-RPC 调用监督
进程,进程按需启动,崩溃时自动重启一次,超预算自动禁用(§10.4);新增
**真实 Go 示例插件端到端测试**(`examples/plugins/sgf-exporter`:go build
真实二进制 → supervisor 启动 → JSON-RPC 往返,无 go 时跳过)。
**迭代 18（发布流水线补全）已完成：** `scripts/bundle-linux-appimage.sh`
（linuxdeploy 收集动态依赖 + appimagetool 产出 AppImage,FUSE-free CI）;
`scripts/installer.nsi`（NSIS 安装器:开始菜单/桌面快捷方式、卸载器、注册
表项;路径经 ROOT_DIR 宏解析,已实测 CI 产出 `ryusei-setup-0.1.0.exe`）;
release.yml 增 Linux AppImage 步骤、Windows NSIS 步骤与 `publish` job
（v* tag 时 softprops/action-gh-release 自动创建 Release 附加全部产物）。
手动触发实测:三平台 job 全绿,产物齐全（macOS .app/dmg、Linux
tar.gz/AppImage、Windows zip/setup.exe）。

**长期目标（2026-08 设定）：** 接近 Sabaki 原版界面：三栏主布局
（左引擎/GTP 栏 + 中央棋盘/模式栏 + 右胜率图/GameGraph/注释栏）、
可拖拽分栏、原版菜单/抽屉/主题/输入体验。完整阶段计划与可借鉴项目
（`gpui-component`/`open-gpui`/`gpui-mullion`/Zed 上游）见
[`docs/ui-parity-plan.md`](ui-parity-plan.md)。

当前 M0–M5 UI 对齐迭代均已落地。后续停止扩展非阻断功能，按
[`release-remediation-plan.md`](release-remediation-plan.md) 推进：

1. **P0 主线收敛**：拆分当前大型工作树，修复 fmt、workspace doctest、clippy
   与 locked release build；
2. **P1 候选证据**：当前 commit 的三平台 CI、全格式打包、真实引擎 smoke、
   Electron 参考并行 QA、干净机器安装/升级/回滚；
3. **P2 架构修复**：拆分 ShellApp，建立 Engine/Analysis/Plugin Controller，
   恢复 KataGo/Fox 网络与文件副作用的 Adapter seam；
4. **P3 正式发布**：macOS Developer ID + notarization、Windows Authenticode、
   Beta 反馈收敛和稳定版门槛验证。

## 7. 技术债 / 已知限制

- 引擎真实进程路径已有真实子进程冒烟测试（fake-gtp-engine.py），但尚未用
  KataGo/GNU Go 实物引擎做过手工验证（需用户配置引擎 + 模型）。
- 原生插件监督与命令调用链已闭环（GPUI 命令按钮经 Supervisor RPC 调用
  真实 Go 示例插件,含崩溃自动重启一次）;WASM 与 native 两条插件路径均
  可执行。
- WASM capability imports 已实现（gameRead→`ryusei.game_snapshot`，未授权
  link 失败）；事务写入（gameWrite）等其余能力未接。
- 插件 `contributes.panels` 已真实渲染；`contributes.settings` 尚未接入
  设置面板，`contributes.menus` 尚未接入应用菜单。
- 原生文本输入已接入 Unicode 字符选区、UTF-16 replacement、undo/redo 与
  CommentBox/节点标题输入；仍需在候选版本中验证各平台 IME 和剪贴板体验。
- WinrateGraph、GameGraph 与 CommentBox 已形成右栏闭环，原生 screenshot/golden
  测试仍受 GPUI 0.2.2 能力限制。
- `sound.enable` 已接入 UI-local `SoundSink`；macOS 使用系统声音，Windows/Linux
  目前为静音 Adapter，捕子/终局 cue 仍等待 Host 语义事件。
- 主题 schema v2 已提供 shell semantic colors，schema v1 主题保留 fallback；材质和
  尺寸 token 可在 Beta 后扩展。
- `MemorySettingsPersistence`/`MemoryHostPersistence`/`MemoryPluginPersistence` 保留供测试，
  生产路径已全部走 Native 实现。
- `main.rs` 已拆分（`panels.rs`），但 `ShellApp` 状态字段与事件处理器仍集中在
  main.rs；后续按需继续细化。
- gpui 0.2.2 无公开窗口激活事件，外部文件检查以帧调度 + `is_active` + 节流实现
  （窗口激活会触发 refresh 帧）；无原生对话框 API，经 rfd 实现。
- 计分已闭环（覆盖事务 + 死子启发 + 区域计分 + GPUI 摘要）;死子判定为
  无气启发式,Monte Carlo 估算（`score.estimator_iterations`）未用。
- `decode_legacy_bytes` 的编码候选顺序（UTF-8→Shift_JIS→EUC-JP→GBK→Big5）
  对字节重叠的多编码文本（如 GBK 与 Shift_JIS 共用字节对）按固定顺序确定性
  决策，可能选错编码（与上游 jschardet 统计检测同为不确定性）；中文 NGF
  建议另存为 UTF-8 后再导入。
- 流式分析已复用已连接会话;胜率条已按行棋方换算（迭代 22）;分析命令支持
  参数透传（迭代 23）。
- Tauri 冻结，其未完成项（theme/plugin 错误 DTO、原生 e2e）不再安排。
