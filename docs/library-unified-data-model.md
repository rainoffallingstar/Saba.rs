# 棋谱库统一数据模型改造方案

> 状态：设计草案（RFC）
> 范围：`crates/domain-core`、`crates/ryusei-host`、`apps/ryusei-gpui`
> 目标：把当前"Git 同步导入器"式的棋谱库，升级为覆盖本地 SGF / OGS / 野狐 / 直播 / 自动保存 / 对局历史的统一棋谱库，并支持**可选的本地数据保存路径**。

---

## 1. 背景与现状

### 1.1 现状盘点

当前棋谱库（`crates/ryusei-host/src/sgf_library.rs`）是一个**受许可门控的 Git 同步导入器**，能力只有三件事：

| 能力 | 实现 | 局限 |
|---|---|---|
| 来源校验 | `SgfLibrarySource::validate_for_sync` | 只接受 `https://github.com`、必须声明再分发权 + 许可证证据 |
| clone/fetch | `sync_sgf_library` | 单向拉取，无 push |
| 扫描 | `scan_sgf_library` | 递归找 `.sgf`，上限 1 万，跳过 `.git`/符号链接 |

UI 侧（`panels/drawers.rs` `render_library_drawer`）是一个抽屉：来源配置表单 + "保存并同步" + 平铺 SGF 列表（`take(200)`，无搜索/标签/目录树）+ "最近打开"。

### 1.2 断裂点（与各功能的契合度）

| 功能 | 现状 | 与棋谱库的关系 |
|---|---|---|
| **OGS 同步** | `ogs_client.rs`/`ogs_socket.rs` 实时对弈，对局**只存内存 session** | 无落盘、无入库，零交集 |
| **野狐同步** | `fox_kifu.rs` 拉列表 + SGF，`apply_fox_game_result` 只 `restore_from_sgf` 载入当前会话 | 一次性导入，不入库 |
| **直播流** | `apply_public_live_capture` 载入只读 Live 会话 + 轮询 | 不落盘、不入库 |
| **本地 SGF** | `recent_files.rs`（10 条 MRU）+ `workspace_tabs.rs`（会话快照） | 与棋谱库无关，无本地目录来源 |
| **GitHub 同步** | 棋谱库核心功能 | 唯一闭环，但只服务"专业棋谱合集" |
| **自动保存** | `autosave.rs` 单槽崩溃恢复；`auto_save` 注释声称"records it in the library history"但实际只 `record_recent` | 无版本历史，不喂棋谱库 |
| **对局历史** | 无此功能；最接近的是 `workspace_tabs`（会话恢复）与 `recent_files`（MRU） | 缺失 |

### 1.3 根因

这些是**互相独立的子系统，没有统一的数据模型**：

- 棋谱库 = git 拉取的文件集合（`SgfLibraryEntry`）
- OGS / 野狐 / 直播 = 内存会话（`restore_from_sgf` 载入即用）
- 自动保存 = 单槽崩溃恢复
- 历史 = 10 条 MRU + 会话快照

它们之间**没有共享的"棋谱入库 / 检索 / 归档"层**。

---

## 2. 目标

1. **统一数据模型**：所有来源（本地 / Git / OGS / 野狐 / 直播 / 自动保存）都归一为同一个 `GameRecord`，带稳定 id、来源溯源、内容、可选本地路径、可检索元数据。
2. **可选本地数据保存路径**：棋谱库支持一个**可配置的本地保存目录**（默认 `<配置目录>/libraries`），任何对局（含 OGS / 野狐 / 直播导入）都可"保存到棋谱库"，并进入统一索引。
3. **统一索引与检索**：一个 `LibraryIndex` 同时覆盖 git 同步来源 + 本地保存目录，支持按标签 / 棋手 / 结果 / 日期检索。
4. **自动保存 / 历史接入**：把单槽崩溃恢复升级为**版本化历史**，作为 `GameRecord` 的 `history` 一部分。
5. **不破坏现有行为**：git 同步、recent files、workspace tabs、autosave 的既有语义保持兼容，增量迁移。

---

## 3. 统一数据模型

### 3.1 `GameRecord`（核心记录）

放在 `crates/domain-core`（UI 无关、可测试），`serde` 可序列化。

```rust
/// 一条统一棋谱记录。所有来源最终都归一为它。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRecord {
    /// 稳定不透明 id（如 `rec-<timestamp>-<counter>`），不暴露路径。
    pub id: String,
    /// 展示标题（取自 SGF 的 GN/PB/PW 或来源名）。
    pub title: String,
    /// 来源溯源（见 3.2）。
    pub source: RecordSource,
    /// 规范内容（UTF-8 SGF 文本）。
    pub sgf: String,
    /// 可选本地保存路径。`None` 表示仅 git 同步或内存来源。
    pub local_path: Option<PathBuf>,
    /// 入库时间 / 最后更新时间（unix 毫秒）。
    pub created_at: u64,
    pub updated_at: u64,
    /// 用户标签（可检索）。
    pub tags: Vec<String>,
    /// 从 SGF 根属性提取的可检索元数据。
    pub metadata: RecordMetadata,
    /// 版本化历史（见 3.4）。
    pub history: Vec<RecordRevision>,
}
```

### 3.2 `RecordSource`（统一来源溯源）

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RecordSource {
    /// 本地文件（含用户手动保存到棋谱库目录）。
    Local { path: PathBuf },
    /// Git 同步来源（保留现有 `SgfLibrarySource` 语义）。
    Git { source_id: String, relative_path: String },
    /// OGS 对局。
    Ogs { game_id: u64 },
    /// 野狐对局。
    Fox { chess_id: String },
    /// 直播（OGS 公开页 / 直播页）。
    Live { page_url: String },
    /// 自动保存恢复。
    Autosave { revision: u64 },
}
```

> 设计要点：`RecordSource` 用 `#[serde(tag = "kind")]` 内部标签，保证向后兼容——旧 `SgfLibraryEntry` 可无损映射为 `RecordSource::Git`。

### 3.3 `RecordMetadata`（可检索元数据）

从 SGF 根属性（`GN`/`PB`/`PW`/`RE`/`DT`/`KM`/`RU`/`HA`/`EV`/`RO`）提取，供索引与检索使用：

```rust
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordMetadata {
    pub black: Option<String>,
    pub white: Option<String>,
    pub result: Option<String>,
    pub date: Option<String>,
    pub event: Option<String>,
    pub round: Option<String>,
    pub komi: Option<f64>,
    pub rules: Option<String>,
    pub handicap: Option<u8>,
    pub board_size: Option<u8>,
}
```

### 3.4 `RecordRevision`（版本化历史）

把自动保存从"单槽"升级为"多版本"，作为记录历史的一部分：

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordRevision {
    pub revision: u64,
    pub sgf: String,
    pub saved_at_unix_milliseconds: u128,
    /// 触发来源：手动保存 / 自动保存 / 导入。
    pub trigger: RevisionTrigger,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RevisionTrigger {
    ManualSave,
    Autosave,
    Import,
}
```

---

## 4. 可选本地数据保存路径

### 4.1 配置

新增设置项（`settings.rs`）：

```
library.local_root   # 可选；默认 <配置目录>/libraries
library.auto_save_to_library  # 可选；默认 false
```

- `library.local_root` 为空 = 不启用本地保存（仅 git 同步，保持现状）。
- 非空 = 启用本地保存目录，`GameRecord::local_path` 落在此目录下。

### 4.2 目录布局

```
<local_root>/
  index.json          # LibraryIndex 持久化（见 5）
  records/            # 本地保存的 SGF 文件
    <category>/<name>.sgf
  git/                # 现有 git 同步来源（迁移自 <配置目录>/libraries/<source_id>）
    <source_id>/...
```

> 迁移：现有 `libraries/<source_id>` 目录在启用 `local_root` 后迁移到 `local_root/git/<source_id>`，或保持原位并让索引同时扫描两处（见 5.3 兼容策略）。

### 4.3 "保存到棋谱库"动作

新增统一动作 `save_to_library`，任何会话（本地 / OGS / 野狐 / 直播）都可调用：

```rust
fn save_to_library(&mut self, cx) {
    let snapshot = self.host.snapshot();
    let sgf = self.host.to_sgf();
    let record = GameRecord::from_snapshot(snapshot, sgf, self.session_policy.source);
    // 1. 若 local_root 未配置 → 提示并引导配置
    // 2. 写入 <local_root>/records/<category>/<name>.sgf
    // 3. 更新 LibraryIndex 并持久化
    // 4. 记录 recent file
}
```

- 对 OGS / 野狐 / 直播：`save_to_library` 是**可选**动作（用户点"保存到棋谱库"或开启 `auto_save_to_library`）。
- 对本地会话：若当前 `file_state.path` 已在 `local_root` 内，则自动纳入索引；否则由用户决定是否复制入库。

---

## 5. 统一索引与检索

### 5.1 `LibraryIndex`

```rust
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIndex {
    /// id → 记录（含 git 同步 + 本地保存）。
    records: BTreeMap<String, GameRecord>,
    /// 标签 → 记录 id 列表。
    by_tag: BTreeMap<String, Vec<String>>,
    /// 棋手名 → 记录 id 列表。
    by_player: BTreeMap<String, Vec<String>>,
    /// 来源 id → 记录 id 列表（git 来源分组）。
    by_source: BTreeMap<String, Vec<String>>,
}
```

### 5.2 检索接口

```rust
pub struct LibraryQuery {
    pub text: Option<String>,      // 标题 / 棋手 / 事件子串
    pub tag: Option<String>,
    pub player: Option<String>,
    pub source: Option<RecordSourceKind>,
    pub date_from: Option<u64>,
    pub date_to: Option<u64>,
    pub limit: usize,              // 默认 200，可翻页
}

impl LibraryIndex {
    pub fn query(&self, query: &LibraryQuery) -> Vec<&GameRecord>;
    pub fn upsert(&mut self, record: GameRecord);
    pub fn remove(&mut self, id: &str);
    pub fn rebuild_from_scan(&mut self, scan: Vec<GameRecord>);
}
```

### 5.3 索引构建

`refresh_library_entries`（现 main.rs L7653）升级为 `rebuild_library_index`，统一扫描：

1. **git 来源**：对每个 `SgfLibrarySource` 调 `scan_sgf_library`，映射为 `RecordSource::Git`。
2. **本地保存目录**：扫描 `local_root/records/**/*.sgf`，映射为 `RecordSource::Local`。
3. **合并**：`rebuild_from_scan` 去重（按 `source` 溯源 + 内容指纹），写入 `index.json`。

> 兼容：`scan_sgf_library` 保留不动，作为 git 来源的底层扫描器；新增 `scan_local_records` 扫描本地目录。两者都产出 `GameRecord`，由 `LibraryIndex` 统一承载。

---

## 6. 各来源接入

### 6.1 OGS 接入

- 对局结束（`phase` 进入终局 / 死子确认完成）时，若 `auto_save_to_library` 开启，自动 `save_to_library`。
- 否则在 OGS 会话提供"保存到棋谱库"按钮。
- `RecordSource::Ogs { game_id }` 保留溯源，便于回链 OGS 页面。

### 6.2 野狐接入

- `apply_fox_game_result`（main.rs L1652）在 `restore_from_sgf` 成功后，追加可选 `save_to_library`。
- `RecordSource::Fox { chess_id }` 保留 chess_id 溯源。

### 6.3 直播接入

- `apply_public_live_capture`（main.rs L1342）载入后，提供"保存到棋谱库"（只读 Live 会话允许导出入库）。
- `RecordSource::Live { page_url }` 保留页面溯源。

### 6.4 本地 SGF 接入

- 打开本地文件时，若路径在 `local_root` 内，自动纳入索引。
- 手动"保存到棋谱库"把任意本地文件复制入库。

### 6.5 GitHub 同步接入

- 保持现有 `sync_sgf_library` 不变，仅把扫描结果映射为 `RecordSource::Git` 并入统一索引。

---

## 7. 自动保存 / 对局历史接入

### 7.1 版本化自动保存

- 保留现有单槽 `AutosaveStore` 作为**崩溃恢复**（启动时提示恢复，语义不变）。
- 新增**版本化历史**：每次 `auto_save` / 手动保存时，向当前 `GameRecord.history` 追加一个 `RecordRevision`（`trigger: Autosave` / `ManualSave`），并持久化到 `index.json`。
- 历史有上限（如每记录保留最近 50 个 revision），防止无限增长。

### 7.2 对局历史

- 统一索引本身就是"对局历史"：按时间倒序列出所有入库记录，支持检索。
- 导航轨的"棋谱库"抽屉升级为：**统一索引列表（可检索）+ 来源分组 + 标签过滤 + 最近打开**。
- 新增"历史"视图 = `LibraryIndex.query` 按 `updated_at` 倒序，替代现在只有 10 条 MRU 的"最近打开"。

---

## 8. 持久化边界

扩展 `HostPersistence` trait（`persistence.rs`），新增库操作：

```rust
pub trait HostPersistence {
    // ... 现有 autosave / recent_files / workspace_tabs ...

    /// 加载统一索引（缺省返回空索引）。
    fn load_library_index(&self) -> Result<LibraryIndex, String> {
        Ok(LibraryIndex::default())
    }

    /// 持久化统一索引。
    fn persist_library_index(&self, index: &LibraryIndex) -> Result<(), String>;

    /// 把一条记录写入本地保存目录（可选路径）。
    fn save_record_to_library(&self, record: &GameRecord) -> Result<PathBuf, String>;

    /// 读取一条记录（按 id 或路径）。
    fn load_record(&self, id: &str) -> Result<GameRecord, String>;
}
```

- 生产实现：`index.json` 写 `local_root`，SGF 写 `local_root/records/...`。
- 测试实现：内存 `RefCell<LibraryIndex>`，与现有 `MemoryHostPersistence` 一致（hermetic）。

---

## 9. 迁移与兼容

| 现有能力 | 迁移策略 |
|---|---|
| `SgfLibrarySource` / `SgfLibraryEntry` | 保留；`SgfLibraryEntry` 映射为 `RecordSource::Git` |
| `scan_sgf_library` | 保留，作为 git 来源底层扫描器 |
| `recent_files` | 保留；统一索引列表可替代"最近打开"展示，但 MRU 语义不变 |
| `workspace_tabs` | 保留；会话恢复与棋谱库索引互不干扰 |
| `autosave` 单槽恢复 | 保留；新增版本化历史作为补充 |
| `libraries/<source_id>` 目录 | 启用 `local_root` 后迁移到 `local_root/git/<source_id>`，或双路径扫描 |

**兼容原则**：所有新增都是**增量**的。`library.local_root` 未配置时，行为与现状完全一致（仅 git 同步）。

---

## 10. 分阶段实施

### Phase 1 — 数据模型（domain-core）
- 新增 `GameRecord` / `RecordSource` / `RecordMetadata` / `RecordRevision` / `LibraryIndex` / `LibraryQuery`。
- 单元测试：序列化往返、`RecordSource` 内部标签、`LibraryIndex` upsert/query/去重。

### Phase 2 — 本地保存路径（ryusei-host）
- 新增 `scan_local_records`、`save_record_to_library`、`load_record`。
- 扩展 `HostPersistence` + 生产/测试实现。
- 测试：写盘 / 读盘 / 目录布局 / 迁移。

### Phase 3 — 统一索引接入（ryusei-host + gpui）
- `rebuild_library_index` 统一扫描 git + 本地。
- 升级 `render_library_drawer`：可检索列表 + 来源分组 + 标签过滤。
- 新增 `save_to_library` 动作。

### Phase 4 — 来源接入（gpui）
- OGS / 野狐 / 直播接入 `save_to_library`。
- 版本化自动保存接入 `GameRecord.history`。

### Phase 5 — 对局历史视图
- 导航轨新增"历史"视图 = 统一索引按时间倒序。

---

## 11. 测试策略

- **domain-core**：`GameRecord` 序列化往返、`RecordSource` 标签兼容、`LibraryIndex` 检索（文本/标签/棋手/日期/来源）、去重。
- **ryusei-host**：`scan_local_records`（含 `.git`/符号链接跳过、上限）、`save_record_to_library` 写盘、`HostPersistence` 失败回滚（沿用现有 `MemoryHostPersistence` 模式）。
- **gpui**：`save_to_library` 动作、`rebuild_library_index` 合并、抽屉渲染（沿用现有 panels 测试模式）。
- **回归**：现有 `sgf_library.rs` 测试全部保留，确保 git 同步语义不变。

---

## 12. 开放问题

1. **`local_root` 默认值**：默认 `<配置目录>/libraries` 是否合适？还是默认关闭（保持现状）更稳妥？
2. **版本历史上限**：每记录 50 个 revision 是否够？是否需要按磁盘占用动态裁剪？
3. **去重键**：按 `source` 溯源 + 内容指纹去重，是否足够？是否需要 SGF 规范化（忽略注释/标记差异）？
4. **索引规模**：git 来源上限 1 万文件，加上本地记录后，`index.json` 是否需要分片或惰性加载？
5. **OGS 自动入库**：`auto_save_to_library` 默认关闭，避免未经同意把对局写盘——是否合理？

---

## 13. 实施状态（TDD 落地记录）

> 本节记录按上文方案落地后的实际状态，与 §3–§8 的设计逐条对照，标注哪些已实现、哪些仍属后续。

### 已落地（本仓库工作区，全部带测试）

- **统一领域模型**（`crates/domain-core/src/library.rs`，9 个集成测试）：
  - `RecordId`（由 `RecordSource` 溯源派生的稳定 id）、`RecordNumber`（稳定编号）。
  - `RecordSource`：`Local / Git / Ogs / Fox / Live`，`#[serde(tag = "kind")]` 序列化。
  - `RecordMetadata`：GN/PB/PW/RE/DT/EV/RO/KM/RU/HA/SZ 全字段归一化，贴目用字符串表示。
  - `GameRecord` + 有界 revision 引用（`RecordRevisionRef` / `RevisionTrigger`）。
  - `LibraryIndex`：稳定编号分配、按来源身份去重、`LibraryQuery`（文本/棋手/结果/来源/标签/排序）、JSON 往返保留编号与 `nextRecordNumber`、`push_revision` 有界（保留最新 N 条）。
- **SGF 根属性解析收敛**：删除手写 `extract_root_properties`，改为复用权威 `SgfParser::parse_root_properties`（消灭双解析器漂移）。
- **host 数据模型收敛**（`ryusei-host/src/sgf_library.rs`）：删除 host 自有 `LibraryEntryMetadata`，`SgfLibraryEntry.metadata` 直接用 domain `RecordMetadata`。
- **host 统一库工作流**（`ryusei-host/src/library_store.rs`，11 个测试）：
  - `LibraryStoreIo` seam（load/save index、可选 `local_root`、record/revision 内容读写、任意源文件读取）。
  - `ingest_library_record`（OGS/Fox/Live 抓取内容）、`ingest_git_entry`（Git 扫描项，不复制内容）、`ingest_local_path`（本地文件，canonical 溯源，不复制内容）、`ingest_git_entries`（批量入库，单次原子保存 index，跨重启编号不回收）。
  - `FsLibraryStore`：原子写（临时文件 + rename）、`records/<content-hex>.sgf` 路径安全（敌意 id 无法逃逸）。
  - `append_library_revision`：有界版本历史，本地根开启时快照 `revisions/<id>/<rev>.sgf`；单槽 crash recovery（autosave）保持独立，不入库。
  - 所有工作流失败时回滚内存索引（沿用 `persistence.rs` 的 previous-store 模式）。
- **设置键**（`ryusei-host/src/settings.rs`）：新增 `library.local_root`（NullableString，可选本地保存根）与 `library.auto_save_to_library`（Boolean，默认关闭），带类型校验测试。
- **缩略图纯 seam + 指纹**（`sgf_library.rs`）：
  - `render_thumbnail_png(content)`（纯、无 FS）、`render_library_thumbnail(path)`（typed `SgfLibraryError`）、`render_library_thumbnail_with_fingerprint`（返回内容指纹，供缓存键）。
- **GPUI 展示修正 + 索引接入**（`apps/ryusei-gpui`）：
  - gallery 改为真多列 `flex_wrap` 网格（固定 112px 卡片），不再 `flex_col` 竖排。
  - 缩略图缓存键改为**内容指纹**（`library_thumbnails: HashMap<fp, Image>` + `entry → fp` 映射），文件内容变化不再复用旧盘面图；渲染并发有界（≤8 in-flight），完成后自动泵下一批。
  - `index_library_sources`：打开/刷新棋谱库时把所有 Git 扫描项批量写入 `FsLibraryStore` 持久索引，`ShellApp` 持有 `library_record_numbers`（entry-id → 稳定编号），列表与网格按编号排序、list 显示索引编号——跨重启编号持久化已接入 UI（`panels::drawers::tests` 3 个 headless 通过）。
  - `sync_library` 完成后自动重建索引，新拉取的棋谱在编号序列末尾追加，不复用旧编号。

- **全来源 `save_to_library` UI 接线**（`apps/ryusei-gpui`）：
  - **野狐**：`save_fox_game_to_library`（`main.rs`）在野狐对局列表每行提供「保存到棋谱库」按钮（`plugin_dialogs.rs`），后台拉取 SGF → `ingest_library_record`（`RecordSource::Fox`）写入 `FsLibraryStore` 并刷新索引。
  - **直播**：`render_live_capture_drawer` 新增「保存快照到库」按钮，将当前直播棋谱抓取为 `RecordSource::Live` 快照入库。
  - **OGS 手动保存**：`render_ogs_account_drawer` 在连接对局时新增「保存对局到库」按钮，将当前 OGS 比赛存入 `RecordSource::Ogs`。
  - **OGS 终局自动入库**：当 OGS 对局切入 `finished` 状态，若 `library.auto_save_to_library` 开关开启，自动调用 `save_current_game_to_library` 入库并提醒。
- **设置表单入口暴露**（`apps/ryusei-gpui/src/settings_form.rs`）：
  - `library.local_root`：「棋谱库本地保存路径（留空为默认）」，支持输入自定义目录，为空时回退默认。
  - `library.auto_save_to_library`：「OGS 终局自动入库」，可直接在设置抽屉中勾选切换。
  - `library_base()` 统一样本，当用户配置 `library.local_root` 时全局生效，覆盖刷新、同步及保存路径。

### 仍需后续完成（未在本轮落地）

- 旧版本索引/目录的显式迁移工具（当前 `FsLibraryStore` 找不到 `index.json` 时从零开始，不破坏既有 `<config>/libraries/<source-id>` Git 检出）。
- 窗口/上下文类 GPUI 测试受 GPUI 0.2.2 无 offscreen API 限制，本环境无法运行；相关纯语义由 domain/host seam 测试与 GPUI 纯逻辑测试（3 个通过）覆盖。

### 测试与构建状态

- `cargo test -p ryusei-domain-core`（lib 79 + 集成 27）与 `cargo test -p ryusei-host --lib`（272）全绿。
- GPUI 纯逻辑测试 `panels::drawers::tests`（3 个）与 `settings_form::tests`（9 个）可 headless 运行，全部通过。
- `cargo check --workspace` / `cargo build -p ryusei-gpui` 通过，`cargo fmt --check` 干净。
