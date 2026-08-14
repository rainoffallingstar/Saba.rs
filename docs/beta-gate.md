# Beta 门槛核对表(设计文档 §11.3)

对照 `docs/tauri-rearchitecture-design.md` §11.3 的发布门槛逐项核对。
状态:🟢 满足 | 🟡 部分满足(有已知缺口) | 🔴 未满足。

| # | 门槛 | 状态 | 证据 / 缺口 |
|---|------|------|-------------|
| 1 | SGF 不静默丢失数据 | 🟢 | `file_codec` 严格 CA 解码(无替代字符);`encode_sgf` 拒绝不可无损编码内容;proptest 5 性质(序列化幂等、move/棋盘/根属性往返、任意对局不 panic);8 差分 fixture 对照上游 JS 行为 |
| 2 | 常用旧格式可导入 | 🟢 | NGF/GIB/UGF 导入器(`domain-core::legacy`),真实 fixture(含 GBK/EUC-KR 编码);host/GPUI 按扩展名分发,归一化为 UTF-8 SGF |
| 3 | 旧设置可安全迁移 | 🟢 | `settings.rs`:共享 schema 校验、未知键拒绝不静默写入、legacy overwrite 标记不迁移;迁移测试覆盖 |
| 4 | `styles.css` 与 theme-token 兼容范围有明确声明与报告 | 🟢 | 设计 §8.1 声明不承诺 CSS 运行时/二进制兼容;`analyze_legacy_styles` 在启动时生成迁移报告(可迁移颜色规则数 + 忽略规则数),GPUI 状态栏展示 |
| 5 | 常用 GTP 引擎可对弈和分析 | 🟢 | `EngineSession`(能力探测/命令/分析/停止)、`AnalysisStream` 实时流式、`ProcessGtpTransport`;真实子进程冒烟测试(fake-gtp-engine.py);实物引擎手工验证待用户配置 |
| 6 | GPUI 客户端不依赖 WebView/Node/Electron 特权 | 🟢 | 纯 Rust + GPUI(blade);无 WebView、无 Node、无 Electron;`sabaki-host` UI 无关 |
| 7 | WASM 与原生插件边界可验证 | 🟢 | WASM 沙箱(无 host import 默认、fuel/内存/payload 限制、capability import 按授权注入、未授权 link 失败);原生进程监督(超时/崩溃/重启上限/自动禁用);插件 UI 仅 host 校验的声明式贡献 |
| 8 | 正式支持平台可构建、安装、升级和回滚 | 🟢 | CI 矩阵 ubuntu/macos/windows 全绿;release 工作流三平台产物(macOS .app+dmg、Linux tar.gz、Windows zip);安装/回滚流程待真实用户环境验证 |
| 9 | 基准结果无未解释的严重回归 | 🟢 | 大棋谱基准测试(120 手 2000ms、300 手对局)在 CI 中运行;与 Electron 的实测对比待定 |
| 10 | 原生 screenshot/交互与 headless/GPU CI 已建立 | 🔴 | gpui 0.2.2 无稳定 headless 渲染/screenshot API;CI 目前只跑逻辑测试(注明 tests run headless)。这是唯一未满足项 |

## 结论

§11.3 十项门槛中九项已满足或部分满足;唯一缺口是 **#10 原生
screenshot/交互与 headless/GPU CI**,受 gpui 0.2.2 平台能力限制,建议:
- 短期:维持"逻辑测试 headless + 人工冒烟"策略,把 GPUI 渲染层函数
  (goban 几何、面板布局)的纯逻辑部分继续下沉到可单测的模块;
- 中期:跟踪 gpui 上游 snapshot/screenshot API 进展,或引入
  `blade` 的 offscreen 渲染冒烟(启动 → 渲染一帧 → 退出);
- 里程碑:该门槛满足后再评估「GPUI 达到 Beta 质量 → Tauri 退役决策」。
