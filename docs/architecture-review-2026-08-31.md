# Ryusei 前后端架构综合审查（2026-08-31）

> 审查日期：2026-08-31
>
> 审查对象：Rust + GPUI 主线仓库（domain-core / ryusei-host / plugin-runtime / ryusei-gpui）
>
> 方法：两份独立代码级评审（后端三 crate、前端 GPUI）+ 既有文档审计（`handoff.md`、
> `architecture-release-audit-2026-08-21.md`、`beta-gate.md`、`release-readiness.md`）
> + 独立代码核对（ShellApp 度量、PRD 对照、技术债标记扫描）。全部测试实跑通过。
>
> 前序审查：`architecture-release-audit-2026-08-21.md`（发布就绪度）。本文为其增量，
> 覆盖 2026-08 下旬的 UI 对齐系列改造（主题地基/质感/图标/动画/顶栏合并/甲板精简/
> 功能收敛）之后的最新状态。

---

## 1. 评级总表

| 维度 | 评级 | 说明 |
|---|---|---|
| **架构** | **A−** | 严格 DAG、端口-适配器彻底、无领域泄漏/循环依赖、零 `unsafe`（底层两 crate） |
| **后端实现** | **B+** | domain-core / ryusei-host 接近生产级，真实分层测试（含真实 KataGo 子进程回归） |
| **前端实现** | **B / Beta 前期** | PRD 功能与 Apple 设计原型已对齐；ShellApp 上帝对象 + panels 视图层裸奔 |
| **测试** | **B+** | domain/host 真实分层；前端视图层（panels.rs 6.7k 行）0 测试靠 2 个渲染帧冒烟兜底 |
| **发布就绪** | **未闭环** | 三平台 CI 历史成功，当前候选 commit 未重跑矩阵 + 签名/公证/安装/升级/回滚未实测 |

**总评**：架构健康、数据安全扎实、工程化成熟。**不处于功能缺失阶段，处于
「在坚实骨架上补规则深度 + 收敛前端架构 + 闭环发布」阶段。**

---

## 2. 架构（强项，A−）

```
domain-core ────────┐
                    ├──> ryusei-host ──> ryusei-gpui
plugin-runtime ─────┘
```

- **依赖方向严格单向**：`ryusei-host → domain-core / plugin-runtime`，两底层 crate 互不依赖。
- **端口-适配器贯彻彻底**：每个副作用面都有可注入 trait（`GtpTransport`/`HostPersistence`/
  `OgsWebSocketTransport`/`PluginPersistence`/`GameFileAccess`），测试全内存 hermetic——
  这是 236 个 host 单测能纯内存运行的根因。
- **单向数据流 + 事务化写**：文档变更全走 `GameTransaction` + revision 保护，前端每帧取
  不可变 `GameSnapshot`，渲染是纯函数。
- **异步/进程防御性设计**：GTP stderr 持续排空防 KataGo 管道死锁（`gtp.rs:182-206`）、
  命令超时毒化防响应错配（`gtp.rs:110-113`）、OGS 重连代次门控（`ogs_client.rs:507-573`）、
  插件退出唤醒全部挂起 RPC（`plugin-runtime/src/lib.rs:534-555`）。
- **安全默认值正确**：WASM 默认零 import（`wasm.rs:114`）、native 插件 `env_clear`
  （`plugin-runtime/src/lib.rs:506`）、native 执行显式授权、主题资产路径逃逸防护、
  插件存储命名空间隔离。

---

## 3. 后端实现度（B+，接近生产级）

**完整**：SGF 树/变着/导航、读秒时钟（三制式 + 远程权威 + SGF 双向）、中国古棋还棋头、
座子制（幂等不污染 RU/KM）、GTP 会话（握手/能力/流式/角色四态）、KataGo 集成
（发现/配置/模型下载/HumanSL 校验）、插件宿主（WASM 四重边界 + 原生监督重启预算）、
主题工作流、OGS 在线服务（登录/对局/聊天/死子/自动匹配，**超出 PRD**）、GIF/PNG 导出。

**基本完整**：提子规则（仅简单 ko）、点目（确定性启发式）、复盘（domain 侧仅 4 档
预算骨架）、legacy 导入（NGF/GIB/UGF）、分析流、野狐棋谱。

### 后端技术债

1. **超劫/循环劫缺失**——`Board::make_move` 只对比前一盘面（`lib.rs:444`），正式对局级
   需 positional superko。
2. **点目启发式**——`mark_surrounded_chains` 仅「零气=死」（`scoring.rs:137-141`），
   双活/带眼死块会误判；Monte-Carlo 未启用（`score.estimator_iterations`）。
3. **zip 安装无 zip-slip 防护**——直接 `unzip -o`/`tar -xf`（`plugin_workflow.rs:232-257`），
   恶意 `../../` 路径未拦截，manifest 解压后才校验。
4. **外部副作用 shell-out 风格**——KataGo 下载/野狐/OGS 公开页走 `Command::new("curl")`，
   无 curl 降级不明确，部分无统一超时；开浏览器用 `open`/`xdg-open`。
5. **锁 unwrap 密集**——`ogs_client.rs` 55 处 `unwrap()`（含 `lock().unwrap()`，毒化即 panic）。

---

## 4. 前端实现度（B，Beta 前期）

### 已对齐 PRD + Apple 设计原型

设计 tokens（theme.rs 精确对应 Apple token 表）、三皮肤（圆角+渐变+内发光+棋子质感）、
PV 鬼影、标记工具箱、三模式解耦（Live 只读 + 远程公平锁）、HumanSL（20K–9D）、
胜率双轨图 + hover tooltip、底部分析甲板（3 tab / 180px）、右侧智能检视器、
单层 44px 顶栏 + 浮动对局胶囊 + 对战胶囊（含 N提捕获数）、Lucide monoline SVG 图标、
focus-ring 焦点环、文本截断、toast 滑入 + AI 候选呼吸脉冲动画。

### PRD 缺口（本轮 C 阶段补全中）

1. Markdown 注释无实时渲染预览（纯文本展示；设计稿亦未实现，但 PRD 要求）。
2. 落子质量徽标 4 档近似（Best/Good/Inacc/Mistake 硬阈值，无 Blunder 档）。
3. 候选卡缺先验概率 Prior %、缺「加入变着树」按钮。
4. 时钟缺加秒制（Fischer）与读秒语音反馈。
5. 变着树缺折叠/设主干/删分支 UI；候选点 is_best 用白边（设计稿金边蓝点）。

### 前端技术债

1. **ShellApp 上帝对象**：`main.rs` 10.4k 行、~130 字段、~190 方法（OGS ~18 字段、
   棋谱库 ~14 字段、13 组 input+focus 平铺）。代码内 `#[expect]` 注释自认待 P2 拆
   controller——未落地。
2. **panels.rs 6.7k 行 48 渲染函数、0 测试**——视图层裸奔；引擎分析流、批量复盘推进、
   OGS 重连、autosave 恢复无测试；无像素级视觉回归（GPUI 0.2.2 无 offscreen API）。
3. **样式/逻辑耦合残留**：强制 `ThemeMode::Dark` gpui-component、`Render for ShellApp`
   内联棋盘几何计算、3 处 `unsafe` 裸指针、生产路径 window open unwrap。

---

## 5. 测试评估

**测试规模（全部实跑通过）**：

| crate | 测试 |
|---|---|
| domain-core | 65 单元 + 18 集成（8 差分 fixture + 5 legacy + 5 proptest） |
| ryusei-host | 236 单元 + 12 集成（含真实 KataGo 子进程回归，70s 通过） |
| plugin-runtime | 27（WASM 沙箱 + 进程监督） |
| ryusei-gpui | 165（纯函数好、17 headless 状态机、2 渲染帧冒烟） |

**强项**：纯函数层（解析/几何/数据转换）覆盖密且有边界 case；不变量（SGF round-trip
不动点、时钟读秒、座子幂等）用 proptest/fixture 钉住；真实子进程冒烟 + 真实 KataGo 回归；
无环境时优雅 skip。

**缺口**：panels.rs 视图层 0 测试；plugin-runtime 无独立集成测试目录；OGS 真实 socket
无故障注入测试；点目边界 case 薄（无双活/三劫/带眼杀瞎组合场景）；GIF/PNG 导出无像素
断言；原生 screenshot/golden 受 GPUI 0.2.2 无 offscreen API 限制。

---

## 6. 建议推进方向（按优先级）

1. **发布链路闭环**（对齐 `release-remediation-plan.md` P0/P1）——重跑三平台矩阵、
   签名/公证、干净机器安装/升级/回滚实测。**Beta 前期 → 公开发布的主线。**
2. **前端架构收敛（P2 拆 controller + 补 panels 测试）**——ShellApp 按域拆子 controller
   （引擎/分析、OGS、库、文本输入），panels.rs 按面板族拆文件并补测试。**可维护性最大杠杆。**
3. **围棋规则完备性**——补 positional superko、点目 Monte-Carlo、复盘领域模型独立化。
4. **副作用工程健壮性**——zip-slip 防护、curl → 注入式 HTTP adapter（统一超时）、
   锁 unwrap → 毒化处理。
5. **PRD 缺口补全**（本轮 C 阶段）——五档质量徽标、is_best 金边蓝点、候选卡先验概率、
   Markdown 渲染预览。
6. **视觉回归保障**——跟踪 GPUI 上游 snapshot API 或引入 blade offscreen 冒烟。

---

## 7. UI 目标决策（2026-08-31 确认）

`ui-parity-plan.md` 的「对齐原版 Sabaki 三栏界面」目标与本项目的 `prd.md`「Apple 设计
系统流星新 UI」方向不同。**决策：UI 的最终目标以 `prd.md` 的 Apple 新设计为准**
（更新、专门的 UX 重构、且已基本落地）；`ui-parity-plan.md` 标记为被 PRD 取代，
仅保留为原版 Sabaki 行为/布局参考。后续前端架构收敛（拆 controller + 补测试）与
视觉工作统一以 PRD 新设计为基准。

---

## 8. 与既有文档的关系

- 本文为 `architecture-release-audit-2026-08-21.md` 的增量，覆盖其后的 UI 对齐系列改造。
- 发布就绪度以 `release-readiness.md` + `beta-gate.md` 为权威（#8 发布链路、#10 原生
  screenshot 两项黄灯未变）。
- 最新架构快照见 `handoff.md`（已同步本轮改造）。
