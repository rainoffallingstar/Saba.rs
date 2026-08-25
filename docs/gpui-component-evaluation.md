# gpui-component 评估

## 结论

建议采用**渐进式引入**，暂不替换现有 GPUI shell、`goban_view`、主题系统或分栏状态管理。

当前 Sabaki 使用 GPUI 0.2.2，并通过根 `Cargo.toml` 的 `[patch.crates-io]` 指向带 macOS 修复的 `vendor/gpui`。`gpui-component` 0.5.1 也声明依赖 GPUI 0.2.2，因此具备名义版本兼容性，但仍需针对本地 patched GPUI 做编译探针。

推荐固定版本：

```toml
gpui-component = "=0.5.1"
```

不要直接跟随上游 git main，也不要让 Cargo 同时解析两份 GPUI。

## 适合优先引入的部分

1. `Button` / `ButtonGroup` / `Badge` / `Tooltip`
   - 底部工具栏
   - 候选点的“试下”和“生成分支”按钮
   - 引擎连接、插件快捷按钮
2. `Select` / `Dropdown` / `Switch`
   - 引擎选择
   - 分析参数
   - 坐标、手数、图表指标设置
3. `List` / `VirtualList`
   - 引擎日志
   - 大型复盘报告
   - 长 variation/property 列表

## 暂不替换的部分

- `goban_view.rs`：gpui-component 不提供围棋棋盘，需要保留自定义棋盘几何、命中测试、候选点、PV、手数和棋谱标记渲染。
- `winrate_graph.rs`：当前图表包含 Sabaki 特有的黑方视角、score lead、稀疏数据、节点点击和实时分析语义，先保留自定义实现。
- `ThemeTokens` / `UiPalette`：应用已有主题包和棋盘专属颜色，不能直接与 gpui-component 全局主题混用。
- 自定义分栏：已有持久化宽度、最小宽度和回归测试，迁移前需要先建立行为等价测试。
- `NativeTextInput`：涉及中文输入法、UTF-16 选择、撤销和焦点语义，未经专项验证不替换。

## 集成风险

- gpui-component 需要初始化 `gpui_component::init(cx)`，并推荐用 `Root` 作为窗口首层视图；这可能与当前 titlebar、popover、drawer、focus 和 toast 体系冲突。
- 依赖树较大，可能增加编译时间、二进制体积和跨平台构建复杂度。
- 本地 patched GPUI 与上游同版本 API 名称相同，不代表行为完全相同。
- 组件主题系统与 Sabaki 主题系统需要显式 adapter，不能各自独立改变颜色和焦点状态。

## 建议验证顺序

1. 在临时 crate 中以本地 `vendor/gpui` + `gpui-component = =0.5.1` 编译探针。
2. 确认 Cargo 只解析一份 GPUI。
3. 先引入无状态 Button、Tooltip、Badge。
4. 通过主题 adapter 接入 `UiPalette`，不引入全局 Root 重构。
5. 再逐个评估 Select、List 和 VirtualList。
6. 最后才考虑 resizable panels、Dialog、Chart 或 Root。

## 参考

- [gpui-component v0.5.1 README](https://github.com/longbridge/gpui-component/blob/v0.5.1/README.md)
- [gpui-component v0.5.1 Cargo manifest](https://docs.rs/crate/gpui-component/0.5.1/source/Cargo.toml)
- [Installation](https://longbridge.github.io/gpui-component/docs/installation)
- [gpui-component releases](https://github.com/longbridge/gpui-component/releases)
