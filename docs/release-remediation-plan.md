# Saba.rs 发布与架构修复计划

> 目标：从当前 GPUI Beta 阶段推进到可重复构建、可安装验证、可公开发布的 `v0.1.0-beta.1`，并为稳定版消除关键架构债务。

## 1. 执行原则

1. 当前根仓库是唯一主线；`refer-repo/` 仅作只读行为参考。
2. 每个阶段结束时必须留下可复现证据：commit、CI URL、产物 checksum、QA 日期与平台。
3. 先恢复门禁，再扩功能；修复期不新增非阻断产品能力。
4. 所有棋谱写入路径以“不静默丢数据”为最高优先级。
5. 重构采用替换而非叠层：新 Controller 接管行为后删除 Shell 中的旧分支。

## 2. P0：形成可审查的主线候选

**预计：2–4 天。**

### P0.1 固化仓库结构

- [x] 将 Saba.rs Git 历史提升到仓库根；
- [x] 将旧 Electron/Tauri Sabaki 移入 `refer-repo/`；
- [x] 将 `/refer-repo/` 加入主线 `.gitignore`；
- [x] 在 README 说明主线与参考仓库的关系；
- [x] 确认根仓库远端与 CI/Tag/Release 权威均为 Saba.rs。

**验收：**

```bash
git rev-parse --show-toplevel
git remote -v
git status --short --ignored
```

`refer-repo/` 必须显示为 ignored，主线远端必须为 Saba.rs。

### P0.2 拆分当前大型工作树

按以下主题拆分提交，避免一个 7,000+ 行不可审查提交：

1. GPUI vendor patches 与回归测试；
2. M1/M2/M3 UI 对齐；
3. 原生输入、主题和声音；
4. Host 分析、计分和复盘；
5. 插件扫描、贡献与示例包；
6. KataGo/Fox 集成；
7. 文档和发布 QA。

每个提交必须独立通过相关 package 测试。

### P0.3 恢复质量门禁

- [x] 执行 `cargo fmt` 结果；
- [x] 诊断 workspace doctest：问题来自并行 Cargo 进程争用 `target/`，串行 gate 连续两次通过；
- [x] 清理未使用 import、dead code 和未处理 `Result`，并以 `-D warnings` 验证；
- [x] 评估 Rust future-incompatibility 报告：仅余上游传递依赖 `block 0.1.6`（uninhabited static）与 `proc-macro-error2 2.0.1`（private `proc_macro` re-export）；二者均非本仓库源码，需跟踪上游升级或有针对性 patch；
- [x] 将 clippy 加入 CI；
- [x] release build 使用 `--locked`。

**验收：**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --locked -p sabaki-gpui
```

四条命令必须连续执行两次均成功。

**本地证据（当前工作树）：** 上述四条命令已在单一串行 shell 中连续两轮成功。每轮包含完整 workspace 单元、集成、属性和 doctest；无 workspace doctest crate 解析错误。`cargo metadata`、`git diff --check` 与打包 shell 的语法检查也通过。

## 3. P1：当前候选版本的三平台证据

**预计：3–5 天。**

### P1.1 CI 与打包

- [x] 推送候选 commit（最终候选：`8880412e54bef87ddf3091e0f9cc830696c067b0`）；
- [x] Ubuntu、macOS、Windows 的 CI 全绿（run [`32693487577`](https://github.com/rainoffallingstar/Saba.rs/actions/runs/32693487577)）；
- [x] 当前 commit 触发 release workflow（run [`32693823374`](https://github.com/rainoffallingstar/Saba.rs/actions/runs/32693823374)）；
- [x] 记录 `.app/.dmg`、tarball/AppImage、zip/NSIS、Flatpak 的 checksum（见 `docs/release-readiness.md`）；
- [ ] 失败时不得引用旧 commit 的成功 workflow 作为证据。

### P1.2 应用身份与版本

统一：

- 产品名；
- 可执行文件名；
- macOS Bundle ID；
- Flatpak app-id；
- Windows uninstall key；
- 配置目录；
- GitHub Release 名称。

将版本号集中到 workspace/package metadata，由打包脚本读取，不再手工硬编码 `0.1.0`。

**验收：** tag、Cargo package、Info.plist、NSIS 和产物文件名版本完全一致。

**本地实现状态：**

- [x] `[workspace.package]` 是唯一版本源，打包脚本以 `scripts/release-version.sh` 读取它（CI tag 以 `APP_VERSION` 覆盖）；
- [x] 产物二进制统一为 `saba-rs`，产品名为 `Saba.rs`；
- [x] macOS/Windows 标识统一为 `dev.saba-rs.app`，Flatpak 使用有效的 `dev.sabars.app`；
- [x] 新配置目录为 `~/.config/saba-rs`，已有 `~/.config/sabaki-gpui` 自动继续使用；
- [x] release workflow 使用 locked build、版本化产物名和 `SHA256SUMS.txt`。

### P1.3 文件关联和安装生命周期

- [x] macOS 注册 SGF/NGF/GIB/UGF Document Types；
- [x] Windows 安装器注册并在卸载时清理文件关联；
- [x] Linux desktop/Flatpak 声明 MIME type；
- [ ] 验证双击棋谱打开（需三平台已安装产物）；
- [ ] 验证新装、覆盖升级、降级、卸载和配置保留（需三平台已安装产物）。

**macOS 本地打包证据：** `scripts/bundle-macos.sh dist/macos-qa` 已成功构建锁定 release、生成 `Saba.rs.app`、通过 `plutil -lint` 和 `codesign --verify --deep --strict`。当前受限本机环境无法由 `hdiutil` 创建 dmg，脚本按设计保留完整 `.app` 并报告 warning；仍需由 CI 或常规 macOS 环境验证 dmg 与安装生命周期。

## 4. P1：真实产品 QA

**预计：3–7 天，可与 P1 打包并行。**

### P1.4 真实引擎 smoke

至少使用 KataGo 和一个第二引擎完成：

1. 配置和启动；
2. 黑/白/分析角色；
3. 对弈和 genmove；
4. 流式分析；
5. stop/detach/reconnect；
6. 保存并重开分析属性；
7. 引擎异常退出与恢复。

记录引擎版本、模型、OS 和结果。

**当前本机 KataGo 证据（2026-08-24）：** macOS 26 / Apple M4 上的 Homebrew
KataGo `1.17.2`（Metal backend）使用 formula 自带
`kata1-b18c384nbt-s9996604416-d4316597426.bin.gz`，完成 GTP handshake、9×9
`boardsize`/`clear_board`、`play B D4` 和 `genmove W`（`F6`）。首次 smoke 暴露
`generate_optimized_gtp_config` 缺少 KataGo 1.17.2 必需的 logging keys，已修正并
重新验证无 unused-config warning。该项只覆盖 play/genmove；流式 analysis、stop、
detach/reconnect、分析属性保存/重开、异常退出与第二引擎仍未完成。

### P1.5 Electron 并行回归

从 `refer-repo/` 选择同一组 fixture，在 Electron 与 GPUI 中执行：

- 多编码打开/保存；
- 变化树与未知属性；
- 标记、注释和节点评价；
- 外部修改冲突；
- 引擎分析；
- 计分；
- 设置迁移。

差异分类：功能缺陷、明确非目标、视觉差异。

### P1.6 macOS 已知高风险路径

每个候选版本必须重新执行：

- fullscreen enter/exit 至少五次；
- 跨显示器和 resize 后退出 fullscreen；
- Open/Save As 对话框期间切换输入法；
- IME、选区和 undo/redo；
- 声音开启/关闭。

## 5. P2：恢复深模块和 IO seam

**预计：1–2 周，可在 Beta 后并行推进。**

### P2.1 Engine/Analysis Controller

- [x] `EngineController<R, T>` 已成为 connected-engine 的深 Module：拥有 role→session map，隐藏 handshake、position replay、raw GTP、genmove、同步广播与 shutdown；`ShellApp`/panels 不再访问 `engine_sessions`；

建立小 Interface：

```text
attach(role, engine)
detach(role)
request_move(role)
start_analysis(request)
stop_analysis()
snapshot()
```

其 Implementation 内部隐藏 session map、replay 和 GTP lifecycle。`AnalysisRunController` 进一步隐藏跨 worker 的 generation invalidation、cooperative stop、dispose/replay request 与 node/player binding；async executor task 与渲染 projection 仍是 GPUI adapter 的责任。Analysis session 已通过 controller 的 lease/return Interface 交接；`ShellApp` 不再直接操作多个 `EngineSession` 或维护原子分析协调状态。

### P2.2 Plugin Controller

- [x] 将 registry、`PluginPersistence` adapter 与 native `Supervisor` map 下沉到 `PluginController<P>`；其 Interface 只公开 restore/install/toggle/grant/authorize/native dispatch/records/process snapshots，成功 mutation 已持久化且 native restart policy 不再泄漏给 `ShellApp`；
- [x] 建立内置 command handler 注册表：`BuiltinPluginCommandRegistry` 集中维护 plugin/command identity，`ShellApp` 只匹配 `BuiltinPluginCommand` 语义值，不再持有 `org.sabaki.*` 字符串；
- [ ] 删除 `ShellApp` 中按插件 ID 扩张的 UI 专有分支（KataGo setup、Fox 导入、position checker、Save dialog）；这些分支已由 registry 分类，但具体 GPUI/Document 副作用 adapter 仍待下沉；
- [x] 第三方 native 插件只通过 controller 的稳定 JSON-RPC 生命周期与 capability/permission 校验工作；WASM 仍按既有 sandbox 协议执行。

### P2.3 Resource Adapter

为以下副作用建立真实 seam：

- [x] Fox HTTP GET：`FoxKifuClient` 仅公开 recent-games/SGF 两项任务，`FoxHttpAdapter` 是内部可替换 seam，`CurlFoxHttpAdapter` 隐藏系统 `curl`；假 adapter 单测覆盖完整 URL/解析路径；
- [x] KataGo 模型下载：`install_katago_model(base, tier)` 隐藏 `curl`，UI 不再直接创建目录或运行系统命令；
- [ ] KataGo 可执行文件发现与引擎资产目录；
- [x] 原子模型下载与失败回滚：同目录临时文件、非空校验、成功后替换；失败/空文件不触碰既有模型。官方发布没有提供本计划可验证的固定 checksum，checksum 校验仍待上游可信 digest 或发布 manifest；
- [ ] 系统命令能力。

Host 提供 `install_katago_model`、`fetch_fox_games` 等任务级操作，不暴露通用 shell 或任意路径能力。

### P2.4 Shell 分解

目标结构：

```text
ShellApp
├── DocumentController
├── EngineController
├── AnalysisController
├── PluginController
├── LayoutState
└── EditorState
```

删除测试越过 Module Interface 的行为。调用方和测试使用同一个 seam。

## 6. P2：本地 GPUI patch 治理

为每个 patch 建立记录：

| Patch | 回归测试 | 上游状态 | 删除条件 |
|---|---|---|---|
| fullscreen drawable resize deferral | fullscreen 状态/resize 回归 | 尚无上游 issue/PR URL | 上游版本实测修复 |
| keyboard layout notification reentrancy | reentrant action 测试 | 尚无上游 issue/PR URL | 上游使用安全 borrow |
| test-platform hooks | frontend smoke | 项目测试需要 | 上游提供等价 seam |

完整 register、基线、触及文件、回归路径与移除条件见
[`gpui-patch-register.md`](gpui-patch-register.md)。

- [ ] patch 拆成独立 commit；当前大工作树不可盲目提交，待 P0.2 分割；
- [ ] 保存上游 issue/PR URL；当前没有已验证 URL，不能伪造上游跟踪；
- [x] 每次 GPUI 升级运行专门检查表：`scripts/verify-gpui-patch.sh` 验证 patch seam、license、locked metadata，register 规定升级/RC 步骤；三平台 CI 在 fmt 前、release matrix 与 Flatpak build 前均强制执行该 guard；
- [x] 清查 vendor license 和必要文件范围：Apache-2.0 license 与五处修改文件记录在 register；

## 7. P3：签名、公证与稳定版

**预计：2–4 周，依赖证书和 Beta 反馈。**

### macOS

- Developer ID Application；
- hardened runtime；
- `notarytool`；
- staple；
- 干净 Mac Gatekeeper 验证。

### Windows

- Authenticode；
- SmartScreen 验证；
- installer publisher 与卸载信息；
- clean VM 安装和升级。

### 稳定版门槛

- [ ] 至少一个公开 Beta 周期；
- [ ] 无已知棋谱数据损坏缺陷；
- [ ] crash/recovery 和外部冲突路径通过；
- [ ] 支持平台均有签名、安装、升级和卸载证据；
- [ ] 真实引擎 smoke 通过；
- [ ] 发布说明明确 Electron 与 GPUI 的功能差异；
- [ ] 配置、主题和插件 schema 冻结或具备迁移策略。

## 8. 暂停新增范围

在 `v0.1.0-beta.1` 前暂停：

- 新的网络平台集成；
- 新插件能力；
- 新主题 schema；
- 新棋谱格式；
- 大型视觉改版。

只接受：

- 数据保真修复；
- 崩溃修复；
- 发布阻断修复；
- 门禁和测试；
- 文档与 QA 证据。

## 9. 里程碑定义

### Alpha-ready

- P0 全部完成；
- 当前 commit 的四条本地门禁全绿；
- 可生成一个未签名开发者包。

### Beta-ready

- P1 全部完成；
- 三平台 CI/打包全绿；
- 真实引擎和 Electron 并行 QA 完成；
- 发布 `v0.1.0-beta.1`，明确非目标和已知限制。

### Stable-ready

- P2 关键风险收敛；
- P3 签名、公证、升级/回滚完成；
- Beta 反馈无高优先级数据损坏或崩溃问题。
