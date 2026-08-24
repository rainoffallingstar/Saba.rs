# Saba.rs 架构与发布就绪度审查

> 审查日期：2026-08-21
>
> 审查对象：Rust + GPUI 主线仓库
>
> 参考实现：本地 `refer-repo/` 中的 Electron/Tauri Sabaki（该目录被主线 Git 忽略）

## 1. 执行摘要

Saba.rs 已经越过技术原型阶段：围棋领域核心、SGF 数据保真、UI 无关 Host 工作流、GTP 引擎会话和插件安全边界均已形成可测试实现；GPUI 客户端具备打开、编辑、分析、计分、保存和插件管理等主要产品路径。

当前最准确的定位是：

- **领域核心与 Host：接近生产级；**
- **GPUI 功能壳层：公开 Beta 前期；**
- **发布工程与版本治理：尚未闭环。**

估算完成度：

| 目标 | 完成度 | 说明 |
|---|---:|---|
| 内部 Alpha | 90% | 修复当前质量门禁并形成干净提交即可 |
| 公开 Beta | 70–75% | 主要剩余当前版本三平台验证、安装 QA 和版本治理 |
| 稳定替代 Electron | 55–65% | 还需签名、公证、升级/回滚、真实用户试运行和 UI 架构收敛 |

## 2. 仓库主线决策

本次整理后，仓库根已切换为 Saba.rs 主线：

```text
apps/                 GPUI 客户端
crates/               domain/plugin/host 模块
examples/             示例引擎与插件
flatpak/              Flatpak 清单
scripts/              打包脚本
docs/                 设计、状态、QA 与修复计划
vendor/gpui/           本地 GPUI 补丁
refer-repo/            旧 Electron/Tauri Sabaki 参考仓库（Git 忽略）
```

`refer-repo/` 保留旧实现及其独立 `.git` 历史，只用于行为、文件格式和 UI 对照。主线代码、Issue、CI、Tag 与 Release 均应以当前根仓库为唯一权威。

## 3. 架构评价

### 3.1 正确的依赖方向

```text
domain-core ───────┐
                   ├──> sabaki-host ──> sabaki-gpui
plugin-runtime ────┘
```

- `domain-core` 不依赖窗口、文件对话框或 GPUI；
- `plugin-runtime` 独立承载插件协议、权限和运行时限制；
- `sabaki-host` 编排 UI 无关工作流；
- `sabaki-gpui` 是生产 Adapter 和原生视图层。

整体不存在明显的反向依赖或循环依赖。

### 3.2 高价值深模块

以下模块通过较小 Interface 隐藏了大量复杂 Implementation，具备良好 leverage 和 locality：

- `crates/sabaki-host/src/file_codec.rs`：SGF `CA` 探测、多种 CJK 编码、严格解码和无损写回；
- `crates/sabaki-host/src/settings.rs`：历史键表、类型校验、加载与持久化回滚；
- `crates/plugin-runtime/src/wasm.rs`：WASM 编译、fuel、内存、payload 和 capability 限制；
- `crates/sabaki-host/src/engine_session.rs`：GTP 生命周期、会话同步与流式分析；
- `crates/domain-core/src/lib.rs`：事务、revision、快照和游戏树一致性。

### 3.3 主要架构风险

#### A. `ShellApp` 成为 God Object

`apps/sabaki-gpui/src/main.rs` 超过 5,000 行，`panels.rs` 超过 3,000 行。`ShellApp` 同时持有文件、恢复、设置、布局、输入、引擎、分析、插件、主题和通知状态。

影响：

- 修改局部行为需要理解整个 Shell；
- 面板直接读取大量 Shell 字段，局部重构困难；
- 异步任务生命周期、状态更新和渲染耦合；
- 编译和测试反馈范围扩大。

建议逐步抽出 `DocumentController`、`EngineController`、`AnalysisController`、`PluginController` 与 `LayoutState`，保持 `ShellApp` 只负责装配和顶层 action dispatch。

#### B. Host IO seam 出现回退

新模块中存在直接平台副作用：

- `katago_setup.rs` 直接调用 `which`、读取目录和写配置；
- `fox_kifu.rs` 直接启动系统 `curl`；
- GPUI `main.rs` 直接下载模型并写文件。

这破坏了“副作用经 Adapter 注入”的 locality。建议增加任务级 `NetworkAccess`、`EngineAssetAccess` 或统一 `ResourceAccess` seam；Host 负责任务工作流，平台 Adapter 负责命令、网络和文件系统。

#### C. 官方插件命令硬编码在 Shell

`on_plugin_command` 根据插件 ID 分支处理 KataGo 和 Fox 功能。继续扩张会使插件协议退化为 Shell 中的 switch statement。

应明确区分：

- 内置扩展 Module；
- 第三方插件协议；
- 声明式 contribution。

内置扩展也应通过注册表或 command handler interface 接入，而不是继续扩大 `ShellApp`。

#### D. 本地 GPUI fork 成为关键供应链依赖

主线通过 `[patch.crates-io]` 使用 `vendor/gpui`，其中包含 fullscreen、文件对话框重入和测试平台修复。这些补丁解决了真实崩溃，但意味着升级和三平台验证成本由项目承担。

每个 patch 必须具备：

1. 最小独立提交；
2. 对应问题说明；
3. 能在旧实现上失败的回归测试；
4. 上游状态记录；
5. 每次 GPUI 升级的保留/删除判定。

## 4. 功能完成度

### 已形成闭环

- SGF 多编码打开、原子保存与无损写回拒绝；
- recovery、recent-files、dirty-close 和外部文件冲突；
- NGF/GIB/UGF 导入；
- 游戏树、变化、标记、线、箭头、注释和 undo/redo；
- GTP 握手、角色、同步、genmove 和流式分析；
- WinrateGraph、GameGraph、CommentBox 和模式栏；
- 计分覆盖、领地估算和整局复盘基础；
- WASM 沙箱、原生插件监督、权限和声明式 Panel；
- 三栏布局、原生输入、主题和 macOS 声音反馈；
- Headless 全窗口布局与输入冒烟测试。

### 可延后到 Beta 之后

- 多棋谱 GameChooser；
- 批量 Clean Markup；
- Advanced Properties；
- 插件 `contributes.settings` 和 `contributes.menus`；
- Windows/Linux 音效及捕子/终局语义音效；
- 像素级 screenshot/golden tests；
- 自动更新器；
- Flatpak 正式应用商店发布。

这些项目应明确列为 Beta 非目标，避免阻塞首个公开测试版本。

## 5. 质量门禁现状

审查期间当前工作树的普通测试结果为：

- `domain-core`、差分 fixture、legacy fixture 和 property tests：通过；
- `sabaki-gpui`：140 个测试通过；
- `sabaki-host`：114 个测试通过；
- `plugin-runtime`：26 个测试通过；
- 真实引擎子进程和组合 workflow：通过。

合计 **341 个普通测试通过**。

但当前仍不能声明 workspace 全绿：

1. `cargo test --workspace` 在 `sabaki-host` doctest 阶段出现依赖 crate 无法找到，单独运行 `cargo test -p sabaki-host --doc` 可以通过，说明存在 workspace/rustdoc 构建不稳定；
2. `cargo fmt --all -- --check` 当前报告格式差异；
3. 构建存在未使用 import、dead code、未处理 `Result` 和未来 Rust 不兼容依赖警告；
4. 当前大型本地改动尚未提交到 `origin/main`，历史 CI 不能证明当前候选版本。

发布候选必须同时通过：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --locked -p sabaki-gpui
```

## 6. 发布工程现状

仓库已有：

- `.github/workflows/ci.yml`：三平台测试；
- `.github/workflows/release.yml`：macOS、Windows、Linux 和 Flatpak 打包；
- macOS `.app/.dmg`、Linux tarball/AppImage、Windows zip/NSIS、Flatpak 脚本。

历史 workflow 曾成功生成全部产物，但当前本地候选改动尚未通过相同矩阵。当前还没有正式 GitHub Release。

### 发布阻断项

- macOS 仅 ad-hoc 签名，没有 Developer ID、hardened runtime、公证和 staple；
- Windows 没有 Authenticode 签名；
- `0.1.0` 分散硬编码于 Cargo、Info.plist、DMG 和 NSIS；
- `Saba.rs`、`sabaki-gpui`、`dev.saba-rs.app`、`dev.sabars.app` 身份不统一；
- SGF 文件关联未在三个安装格式中完整落地；
- 干净机器新装、升级、降级、卸载和配置保留尚未形成证据；
- 尚未用真实 KataGo/GNU Go 完成候选版本手工 smoke；
- 尚未完成同 fixture 的 Electron/GPUI 并行 QA。

## 7. 发布距离判断

| 里程碑 | 预计距离 | 前提 |
|---|---|---|
| 内部 Alpha | 2–4 天 | 形成干净提交，fmt/test/clippy/release build 全绿 |
| `v0.1.0-beta.1` | 1–3 周 | 当前 commit 三平台打包、真实引擎 smoke、干净机器安装 QA |
| 稳定版 | 6–10 周 | 签名、公证、升级回滚、Beta 反馈和关键 UI 架构收敛 |

## 8. 结论

项目剩余距离主要不在棋局核心，而在：

1. 把当前大型本地实现整理为可审查、可追踪的提交；
2. 修复确定性质量门禁；
3. 当前 commit 的三平台构建和安装验证；
4. 版本、应用身份、签名与升级治理；
5. 控制 Shell 膨胀并恢复被绕过的 IO seam。

在这些工作闭环前，可以发布开发者预览，但不应将当前工作树直接标记为稳定版。
