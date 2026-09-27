# Rust 代码风格审计报告：函数式 vs OOP 与最佳实践

> 审计对象：`crates/` 下 9 个 crate、110 个 `.rs` 文件、36,892 行代码
> 审计基准：`rust-skills`（leonardomso/rust-skills v1.5.1，265 条规则 / 26 类），已安装到
> `~/.workbuddy/skills/rust-skills`，源仓库在 `D:\Documents\GitHub\rust-skills-leonardomso`
> 审计时间：2026-09-26　工具链：cargo 1.98.1（rustc edition 2024）

---

## 一、结论速览

| 维度 | 判定 | 说明 |
|------|------|------|
| **表达式层** | ✅ 偏函数式（约 55/100） | 迭代器链、`?` 传播、`map_err`、`let-else` 用得很足 |
| **领域服务层** | ❌ 偏 OOP（约 25/100） | Kotlin `*Service` / `*Manager` 上帝对象 + 构造注入 + 共享可变状态的直译 |
| **错误处理** | ⚠️ 合格但有硬伤 | `thiserror` 用对了，但生产代码有 173 个 panic 路径、错误类型字符串化 |
| **工程门禁** | ❌ 缺失 | CI 无 clippy / 无 test / 无 fmt；`cargo fmt --check` 有 399 处 diff |
| **unsafe** | ✅ 可控 | 仅 3 处（Android logcat FFI），但缺 `SAFETY:` 注释 |

**一句话结论**：这是一份"**用 Rust 语法写出来的 Kotlin 架构**"。数据转换管线（行映射、集合处理、
`Option`/`Result` 组合子）已经相当函数式、写得不错；但 `suwayomi-domain` 的服务层几乎逐字照搬了
上游 `suwayomi.manga.impl.*` 的面向对象结构——`MangaService` / `ChapterService` /
`DownloadManager` / `ExtensionStoreService` 这些"把状态和一堆方法捆在一起的对象"，
加上 `Arc<dyn Trait>` 构造注入，是典型的 Spring/Koin 依赖注入直译，不是 Rust 的惯用法。

默认级别 `cargo clippy` 全 workspace 只有 **1 条 warning**，说明"最低门槛"是守住的；
但 pedantic + nursery 级别有 **2,664 条 warning**——门开得太低，问题都在门槛下面堆着。

---

## 二、审计方法与复现命令

| 步骤 | 命令 | 结果 |
|------|------|------|
| 量化扫描（自写脚本，55 个正则指标） | `python rust_style_scan.py` | 见 §3 |
| 格式门禁 | `cargo fmt --all --check` | **399 处 diff**，涉及 ~60 个文件 |
| 默认 clippy | `cargo clippy --workspace --all-targets` | **1 warning** |
| pedantic + nursery | `cargo clippy --workspace --all-targets -- -W clippy::pedantic -W clippy::nursery` | **2,664 warnings** |
| CI 门禁核对 | `.github/workflows/build.yml` | 只有 `cargo build --release`，**无 clippy / 无 test / 无 fmt** |

统计口径：`total` = 含测试代码的全部命中；`prod` = 排除 `tests/` 目录与内联 `mod tests` 之后的命中。
本报告的问题清单一律以 **prod** 数字为准。

---

## 三、量化指标

### 3.1 OOP 味道（越高越 OOP）

| 指标 | prod 命中 | 热点文件 |
|------|----------:|----------|
| `impl` 块 | 190 | — |
| `struct` 声明 | 576 | — |
| `&self` 方法 | 755 | graphql/types.rs、graphql/query.rs |
| `Arc<dyn …>` 构造注入 | 26 | domain/tracker/mod.rs(5)、domain/manga/manga_list.rs(3) |
| `Arc<Mutex<…>>` / `Arc<RwLock<…>>` | 9 | domain/download.rs、domain/source/sandbox.rs |
| `*Service` / `*Manager` / `*Handler` 命名 | 15 | domain/ 下 13 处 |
| `pub fn new()` 构造器 | 33 | 每个 Service 一个 |
| Java 风格 getter（`pub fn x(&self) -> …`） | 43 | — |
| Java 风格 setter（`pub fn set_x`） | 5 | 其中 3 个是**全局** setter |
| `&mut self` 方法 | 17 | opds/xml.rs(5)、domain/extension_store.rs(5) |
| 全局可变单例（`static` + `OnceLock<RwLock<…>>`） | 3 | core/config、domain/download、domain/source/local |

### 3.2 函数式味道（越高越函数式）

| 指标 | prod 命中 | 评价 |
|------|----------:|------|
| `?` 传播 | 1,140 | ✅ 很好，错误基本靠 `?` 往上传 |
| `.map_err(…)` | 328 | ✅ 好 |
| `.map(…)` | 416 | ✅ 好 |
| `.iter().…` 链 | 146 | ✅ |
| `.and_then(…)` | 130 | ✅ |
| `let … else` | 127 | ✅ 用上了 2024 edition 特性 |
| `.filter(…)` | 76 | ✅ |
| `.fold(…)` | 1 | ⚠️ 几乎不用的归约组合子 |
| `-> impl Iterator / Fn / Future` | **0** | ❌ 完全没有把迭代抽象成可组合的值 |

### 3.3 命令式残留（越高越 OOP）

| 指标 | prod 命中 | 说明 |
|------|----------:|------|
| `let mut` | 434 | 可变局部状态 |
| `.push(…)` | 360 | 累积式循环 |
| 下标访问 `x[i]` | 209 | 带边界检查且可能 panic |
| `while` 循环 | 28 | — |
| `for i in 0..len` 索引循环 | 11 | 应换 `enumerate()` / 迭代器 |
| 本可用组合子却写 `match`/`if-let`（clippy `map_unwrap_or` + `option_if_let_else` + `single_match_else` + `manual_let_else`） | **257** | ❌ 最大的函数式缺口 |

### 3.4 安全与健壮性

| 指标 | prod 命中 | 说明 |
|------|----------:|------|
| `.unwrap()` | 73 | ❌ 约 1/1000 行 |
| `.expect(…)` | 95 | ⚠️ 部分是无意义的断言式文案 |
| `panic!` | 4 | ❌ 其中 1 处在库代码里 |
| `unreachable!` | 1 | — |
| **panic 路径合计** | **173** | ≈ **4.9 处 / 千行生产代码** |
| `unsafe` | 3 | Android logcat FFI，缺 `SAFETY:` 注释 |
| `.clone()` | 508 | 结合 73% 的 struct 派生 `Clone`，疑似"clone 讨好借用检查器" |
| 派生 `Clone` 的 struct | 421 / 576（73%） | ⚠️ 偏高 |
| 未检查的数值 `as` 转换（clippy 5 个 cast lint 合计） | 521 | ❌ 静默截断风险 |
| `#[allow(…)]` | 27（其中 `dead_code` 12） | ⚠️ 用 allow 掩盖问题 |

---

## 四、问题清单（按严重度）

### P0 — 架构层：Kotlin OOP 直译（建议优先重构）

#### P0-1　`Arc<dyn Trait>` 构造注入 —— 把 Spring/Koin 的依赖注入搬进了 Rust

**证据**（26 处 `Arc<dyn …>`）：

```rust
// crates/suwayomi-domain/src/manga/mod.rs:67
pub struct MangaService { pub db: Db, pub fetcher: Arc<dyn SourceFetcher> }
impl MangaService { pub fn new(db: Db, fetcher: Arc<dyn SourceFetcher>) -> Self { … } }

// 同构：chapter/mod.rs:52、manga/manga_list.rs:17、updater.rs:96、download.rs:77
// 全部是 `new(Db, Arc<dyn SourceFetcher>)`
```

```rust
// crates/suwayomi-domain/src/tracker/mod.rs:217 —— 双层 trait object
services: Arc<Vec<Arc<dyn TrackerService>>>,
```

**为什么是问题**：`dyn` 意味着堆分配 + 虚表动态派发；而这里的实现在编译期是唯一确定的
（生产环境只有一个 `HttpSandboxFetcher`）。Rust 的惯用法是**泛型 / `impl Trait` 单态化**，
或者干脆不抽象、直接传具体类型。`Arc<Vec<Arc<dyn …>>>` 更是把"运行时多态"套了两层。

**修复方向**（`anti-type-erasure`）：

```rust
// 方案 A（推荐，零开销）：泛型单态化
pub struct MangaService<F> { db: Db, fetcher: F }
impl<F: SourceFetcher> MangaService<F> { … }

// 方案 B（若确实需要运行时切换实现）：用 enum 表达封闭集合，比 dyn 更快、可穷尽匹配
pub enum Fetcher { Sandbox(HttpSandboxFetcher), Local(LocalFetcher), Disabled }
```

#### P0-2　`*Service` / `*Manager` 上帝对象

**证据**：15 处 `Service`/`Manager` 命名，其中：

| 类型 | 文件 | 规模 |
|------|------|------|
| `ExtensionStoreService` | domain/extension_store.rs | 1,578 行 / 42 个方法 / 6 个字段 |
| `DownloadManager` | domain/download.rs | 1,299 行 / 27 个方法 / **10 个字段** |
| `TrackerManager` | domain/tracker/mod.rs | 823 行 / 41 个方法 |
| `MangaService` / `ChapterService` / `LibraryService` / `CategoryService` … | domain/ | — |

`DownloadManager` 把 10 份状态捆在一个对象里，其中 4 份是共享可变状态：

```rust
// crates/suwayomi-domain/src/download.rs:76
pub struct DownloadManager {
    db: Db, fetcher: Arc<dyn SourceFetcher>, data_dir: PathBuf,
    client: reqwest::Client, server_base_url: String,
    queue: Arc<Mutex<VecDeque<DownloadJob>>>,              // 可变
    progress_by_id: Arc<std::sync::Mutex<HashMap<i32,f64>>>, // 可变
    tx: broadcast::Sender<DownloadEvent>,
    running: Arc<AtomicBool>, worker_spawned: Arc<AtomicBool>, // 可变 ×2
}
```

**为什么是问题**：这就是"对象 = 状态 + 操作状态的方法"的 OOP 定义。状态被切成 4 个
`Arc<Mutex<…>>` 分散在对象内部，任何方法都能改，无法从类型上看出"谁能改什么"。
函数式的做法是**让状态流转显式可见**：数据用不可变结构表示，状态迁移用纯函数
`State + Event -> State` 表达，副作用集中在边界。

**修复方向**（`anti-over-abstraction` 的反面：这里不是过度抽象，是过度封装）：

```rust
// 1) 队列状态抽成不可变快照 + 显式迁移函数
#[derive(Clone, Debug, Default)]
pub struct DownloadQueue { jobs: im::Vector<DownloadJob>, running: bool }

impl DownloadQueue {
    fn apply(self, ev: QueueEvent) -> Self { … }   // 纯函数：旧状态 + 事件 -> 新状态
}

// 2) 只留一份 actor 持有的可变状态（tokio 任务 + mpsc），对外暴露命令与快照
pub struct DownloadActor { cmd_tx: mpsc::Sender<Command>, snap_rx: watch::Receiver<DownloadQueue> }
// 3) 纯逻辑（路径拼装、进度计算、重试判定）拆成自由函数，天然可单测
pub fn chapter_dir(root: &Path, manga: &str, chapter: &str) -> PathBuf { … }
```

#### P0-3　进程级全局可变状态（3 个隐藏单例 + 3 个全局 setter）

**证据**：

```rust
// crates/suwayomi-domain/src/download.rs:22
static DOWNLOADS_ROOT_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
pub fn set_downloads_root(path: Option<PathBuf>) { … }        // :25

// crates/suwayomi-domain/src/source/local.rs:25   —— 同构
static LOCAL_ROOT_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
pub fn set_local_source_root(path: Option<PathBuf>) { … }     // :29

// crates/suwayomi-core/src/config/mod.rs:9        —— 同构
static CACHE_ROOT_OVERRIDE: std::sync::OnceLock<std::path::PathBuf> = OnceLock::new();
pub fn set_cache_root(dir: std::path::PathBuf) { … }          // :12
```

**为什么是问题**：全局可变状态是 OOP 单例模式的变体，它让函数的输出不仅取决于参数，
还取决于"之前有没有人调过 setter"——这正是函数式要消除的隐式依赖。
后果很实际：`ExtensionStoreService::with_dirs` 的文档注释里已经写明，因为
`std::env::set_var` 在 2024 edition 是 `unsafe` 且多线程下是数据竞争，
测试不得不"注入路径"来绕开——也就是说全局状态已经在反噬可测试性了。

**修复方向**：把路径作为参数显式传递，或用一份 `AppPaths` 在启动时构造一次后以
`&AppPaths` 共享（`Arc<AppPaths>` 只读，无需 `RwLock`）。

```rust
#[derive(Clone, Debug)]
pub struct AppPaths { pub data: PathBuf, pub downloads: PathBuf, pub cache: PathBuf, pub local_sources: PathBuf }

pub fn downloads_root(paths: &AppPaths) -> &Path { &paths.downloads }   // 无全局状态、无锁
```

#### P0-4　生产代码的 173 个 panic 路径（≈4.9 处/千行）

| 类别 | prod | 典型证据 |
|------|-----:|----------|
| `.unwrap()` | 73 | 见下 |
| `.expect(…)` | 95 | 见下 |
| `panic!` | 4 | db/row.rs:77、server/build.rs:62 等 |
| `unreachable!` | 1 | — |

```rust
// ① 锁中毒即崩进程 —— domain/download.rs:140,152,156 / source/sandbox.rs:656,679,712
let progress = self.progress_by_id.lock().unwrap();      // :140
*child.lock().unwrap() = Some(c);                        // sandbox.rs:679

// ② 断言式 expect：文案声称"不可能失败"，但没有任何东西保证
//    core/src/auth.rs:246, 252
let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac accepts any key length");
//    domain/extension_store.rs:279、domain/koreader_sync.rs:78
http: builder.build().expect("reqwest client"),

// ③ 库代码里直接 panic —— crates/suwayomi-db/src/row.rs:77
Err(e) => panic!("suwayomi-db: {e}"),

// ④ 用 unwrap 表达"刚刚 contains 过"的不变式 —— domain/category/mod.rs:60
let id = existing.iter().find(|c| c.name.to_lowercase() == lower).map(|c| c.id).unwrap();
```

**为什么是问题**：服务器进程里一次 panic 就是一个请求/线程的死亡，最坏情况是带崩整个下载队列。
`err-no-unwrap-prod` / `anti-unwrap-abuse` / `anti-expect-lazy` 三条规则都直指这一点。
第 ④ 类尤其危险——`contains` 与 `find` 的匹配条件（`to_lowercase()` vs 大小写）一旦出现任何偏差，
就是一个没有上下文的 `unwrap on None`。

**修复方向**：

```rust
// ① 锁：不要用 unwrap 吞掉中毒，显式转成错误（或改用 parking_lot 的无中毒锁）
let progress = self.progress_by_id.lock().map_err(|e| DomainError::invalid(format!("progress lock poisoned: {e}")))?;

// ② 别用 expect 掩盖：new_from_slice 换成返回 Result 的路径并传播
let mut mac = HmacSha256::new_from_slice(&self.secret).map_err(|e| DomainError::invalid(format!("bad hmac key: {e}")))?;

// ③ 库代码永远返回 Result，panic 决策权交给上层
// db/row.rs:77 ->  Err(e) => return Err(DbError::Decode { … }),

// ④ 查一次就够，用 ok_or_else 给出可读错误
let id = existing.iter()
    .find(|c| c.name.eq_ignore_ascii_case(name))
    .map(|c| c.id)
    .ok_or_else(|| DomainError::not_found(format!("category {name}")))?;
```

#### P0-5　字符串化错误类型（`anti-stringly-typed`）

```rust
// crates/suwayomi-domain/src/error.rs
pub enum DomainError {
    Db(#[from] suwayomi_db::Error),
    NotFound(String), Invalid(String), Source(String),
    Sandbox(String),  Tracker(String), TokenExpired(String),
    TrackerNotFound(i32),
}
```

**为什么是问题**：6 个变体都塞一个 `String`，调用方无法结构化地区分"章节不存在"和
"漫画不存在"，只能做字符串匹配；GraphQL 层也只能原样把字符串丢给前端。
另外 `impl From<reqwest::Error> for DomainError` **一律**映射成 `Sandbox`，
意味着 tracker 的 HTTP 失败也会被标成"sandbox 错误"——语义错位，排查时会误导。

**修复方向**：用携带类型信息的新类型变体 + `thiserror` 的 `#[source]` 链：

```rust
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("manga {0} not found")]      MangaNotFound(i32),
    #[error("chapter {0} not found")]    ChapterNotFound(i32),
    #[error("tracker {0} request failed")] TrackerHttp(i32, #[source] reqwest::Error),
    #[error("sandbox request failed")]   SandboxHttp(#[source] reqwest::Error),
    #[error("database error")]           Db(#[from] suwayomi_db::Error),
}
```

---

### P1 — 控制流：命令式写法压过了组合子

#### P1-1　257 处"本可以用组合子 / let-else，却写了 match 或 if-let"

clippy pedantic 的实测分布（`anti-index-over-iter` 同族）：

| lint | 数量 | 含义 |
|------|-----:|------|
| `clippy::map_unwrap_or` | 116 | `.map(f).unwrap_or(x)` → `.map_or(x, f)` |
| `clippy::option_if_let_else` | 76 | `if let Some(v) = … { … } else { … }` → `map_or_else` |
| `clippy::single_match_else` | 42 | 单分支 `match` → `if let` / 组合子 |
| `clippy::manual_let_else` | 23 | `match … { Ok(v)=>v, Err(_)=>return }` → `let Ok(v) = … else { return }` |

热点：`domain/download.rs`（703/713/719/730 连着 4 处）、`domain/source/local.rs`（199/203）、
`rest/routes/image.rs`（60/68）、`core/backup.rs`（390/489/563）、`db/config.rs`（58）。

```rust
// 现状 —— domain/download.rs:703
let source_dirs = match std::fs::read_dir(&root) { Ok(it) => it, Err(_) => return Ok(0) };

// 函数式写法
let Ok(source_dirs) = std::fs::read_dir(&root) else { return Ok(0) };
```

#### P1-2　索引循环与下标访问（209 处）

```rust
// domain/source/local.rs:209, 231, 276 —— 同一个模式出现 3 次
for i in 0..zip.len() {
    let Ok(entry) = zip.by_index(i) else { continue };
    …
}
// db/backend/sqlite.rs:94
for i in 0..columns.len() { … }
```

`zip::ZipArchive` 的 API 确实只能按索引取（`by_index`），所以这里保留 `for i in 0..len`
是**可以接受**的；但 `sqlite.rs:94` 那处没有这个约束，应改成 `.iter().enumerate()`。
更值得改的是 209 处 `x[i]` 下标访问——它们都是运行期边界检查 + 潜在 panic，
优先换成 `.get(i).ok_or(…)?` 或迭代器。

#### P1-3　可变累积器（`let mut` 434 + `.push()` 360）

```rust
// domain/source/local.rs:207-223 —— 命令式：建集合 → 循环 push → 排序 → 再 enumerate
let mut seen = HashSet::new();
let mut names = Vec::new();
for i in 0..zip.len() { …; names.push(name); }
names.sort_by(|a, b| natural_cmp(a, b));
names.into_iter().enumerate().collect()
```

函数式写法（同样是"取名字 → 去重 → 排序 → 编号"的管线）：

```rust
(0..zip.len())
    .filter_map(|i| zip.by_index(i).ok())
    .map(|e| e.name().rsplit('/').next().unwrap_or("").to_string())
    .filter(|n| !n.starts_with('.') && !n.is_empty())
    .filter(|n| IMAGE_EXTS.contains(&n.rsplit('.').next().unwrap_or("").to_lowercase().as_str()))
    .collect::<BTreeSet<_>>()          // 去重 + 有序一次搞定
    .into_iter()
    .enumerate()
    .collect()
```

#### P1-4　零处 `-> impl Iterator / Fn / Future`

全仓库 `impl_trait_ret = 0`。这意味着没有任何一个函数把"一段迭代/一个计算"作为**值**返回出去，
所有数据流都是就地消费完——这正是"过程式管线"和"可组合的函数式抽象"的分界线。
建议从 `source/local.rs` 的分页/过滤逻辑、`db` 的行映射开始试点。

#### P1-5　两个"可变游标对象"应改为纯函数

```rust
// domain/extension_store.rs:1309 —— protobuf 读取器：内部游标 + &mut self
struct PbReader<'a> { d: &'a [u8], i: usize }
impl<'a> PbReader<'a> {
    fn varint(&mut self) -> PbResult<u64> { … self.i += 1; … }
    fn key(&mut self) -> PbResult<(u64, u64)> { … }
    fn skip(&mut self, wt: u64) -> PbResult<()> { … }
    fn len_delimited(&mut self) -> PbResult<&'a [u8]> { … }
}
```

函数式写法：不持有游标，每次返回"解析结果 + 剩余切片"，调用方用 `?` 串起来，
解析过程变成一条纯函数链，且天然零拷贝：

```rust
fn varint(d: &[u8]) -> PbResult<(u64, &[u8])> { … }   // (值, 剩余)
fn key(d: &[u8])    -> PbResult<((u64, u64), &[u8])> { let (k, rest) = varint(d)?; Ok(((k >> 3, k & 7), rest)) }

let ((f, wt), rest) = key(d)?;          // 显式的数据流
```

```rust
// opds/src/xml.rs:22 —— push-based 可变构建器
pub struct XmlWriter { buf: String }
impl XmlWriter { pub fn element(&mut self, …, children: impl FnOnce(&mut Self)) { … } }
```

函数式替代：先构造不可变的 `XmlNode` 树（递归 enum），再由一个纯函数 `render(&XmlNode) -> String`
折叠输出。树可测试、可复用、可差分，`render` 是纯函数。

---

### P2 — API 与工程门禁

| # | 问题 | 证据 | 规则 |
|---|------|------|------|
| P2-1 | CI 无 clippy / 无 test / 无 fmt | `.github/workflows/build.yml` 只有 `cargo build --release` | `lint-clippy-nursery-selected`、`lint-rustfmt-check` |
| P2-2 | `cargo fmt --all --check` 有 **399 处 diff** | 最多：`graphql/mutation_b4.rs`(30)、`domain/extension_store.rs`(29)、`opds/feeds/mod.rs`(25)、`domain/download.rs`(23)、`graphql/query.rs`(22)、`core/backup.rs`(21) | `lint-rustfmt-check` |
| P2-3 | `rustfmt.toml` 写 `edition = "2021"`，workspace 是 `edition = "2024"` | 配置漂移，2024 的部分格式规则不生效 | `lint-cfg-check` |
| P2-4 | 无 `[workspace.lints]`、无 `clippy.toml` | 9 个 crate 各自定义（或完全没有）lint | `lint-workspace-lints` |
| P2-5 | 未检查的数值转换 **521 处** | `cast_possible_truncation`(201)、`cast_lossless`(126)、`cast_possible_wrap`(108)、`cast_sign_loss`(59)、`cast_precision_loss`(27) | `num-*` 5 条 |
| P2-6 | 参数类型过宽：`&String` ×6、`&Vec<…>` ×4、`&PathBuf` ×1 | 应为 `&str` / `&[T]` / `&Path` | `anti-string-for-str`、`anti-vec-for-slice` |
| P2-7 | 转换 trait 缺失：`impl Display` 0、`TryFrom` 0、`AsRef` 0 | 错误类型只能靠 `to_string()` 传出 | `api-common-traits`、`conv-*` |
| P2-8 | `.clone()` 508 次 + 73% struct 派生 `Clone` | 疑似"clone 讨好借用检查器" | `anti-clone-excessive` |
| P2-9 | `#[must_use]` 只写了 1 个，clippy 认为有 350 个候选 | 返回值被丢弃不告警 | `api-must-use` |
| P2-10 | 文档缺口：`doc_markdown` 365、`missing_errors_doc` 342、`missing_panics_doc` 22 | — | `doc-*`、`err-doc-errors` |
| P2-11 | 3 处 `unsafe` 无 `SAFETY:` 注释 | `server/src/lib.rs:743`（`extern "C"`）、`:754`（调用）、`server/src/main.rs:35` | `lint-unsafe-doc` |
| P2-12 | `format!` 541 处，其中 54 处是把 `format!(…)` 追加进 `String` | 应 `write!` / `push_str` | `mem-avoid-format`、`mem-write-over-format` |
| P2-13 | `#[allow(…)]` 27 处（含 `dead_code` 12） | 用 allow 掩盖而非清理 | `lint-deny-correctness` |
| P2-14 | 测试覆盖不均：110 个文件中 **76 个没有任何测试**；仅 7 个 `tests/` 集成测试文件（1,523 行） | `graphql/`、`rest/` 大部分无测试 | `test-*` |
| P2-15 | `too_many_lines` 30 处函数超 100 行 | `db/dialect.rs:45`(116行)、`core/backup.rs:378/728`、`domain/download.rs:301/632` | `proj-*`、`lint-warn-complexity` |
| P2-16 | `unused_async` 30 处 + `unused_async_trait_impl` 88 处 | 标了 `async` 却没有 `.await`，白付状态机开销 | `async-fn-in-trait` |
| P2-17 | `use_self` 104 处、`redundant_closure_for_method_calls` 58 处 | 可读性细节 | `name-*`、`closure-*` |

---

## 五、做得好的地方（值得保持）

1. **错误传播**很干净：`?` 1,140 次、`map_err` 328 次、`ok_or` 75 次——基本没有"吞错误"的写法。
2. **`thiserror` 用对了地方**：`suwayomi-core/backup.rs`、`suwayomi-db/error.rs`、`suwayomi-domain/error.rs`
   都有结构化错误枚举（`err-thiserror-lib`）。
3. **2024 edition 特性跟得上**：`let-else` 127 处、`if let` 链、let-chains（`download.rs:34-41`），
   说明代码是跟着工具链演进的，不是停在 2018 的写法。
4. **`unsafe` 极少**：3 处，且都局限在 Android logcat 的 FFI 边界（`server/src/lib.rs`），
   没有把 `unsafe` 散到业务逻辑里。
5. **默认 clippy 干净**：全 workspace 1 条 warning，说明基本正确性没有问题。
6. **领域模块拆分合理**：`category / chapter / manga / meta / page / source / tracker`
   按领域切分，不是按技术层切分——这点比很多 Rust 项目都好。
7. **注释密度不低**：生产代码 1,611 行 `///` 文档注释，关键模块（extension_store 84 行、
   local.rs 74 行、sandbox.rs 51 行）解释充分，并且主动写明了"为什么绕开 env var"这类设计权衡。

---

## 六、改进路线图

### 阶段 1 —— 先把门禁立起来（1～2 天，纯配置，零风险）

```toml
# rustfmt.toml
edition = "2024"        # 与 workspace 对齐
max_width = 120
use_small_heuristics = "Max"

# 根 Cargo.toml
[workspace.lints.rust]
unsafe_code = "warn"
missing_docs = "warn"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
panic = "warn"
indexing_slicing = "warn"
cast_possible_truncation = "warn"
# 先 warn 后 deny，给存量代码一个过渡期
```

```bash
cargo fmt --all                      # 一次性吃掉 399 处 diff
```

CI（`build.yml`）里加两个 job：`fmt`（`cargo fmt --all --check`）、
`clippy`（`cargo clippy --workspace --all-targets -- -D warnings`）、`test`（`cargo test --workspace`）。

### 阶段 2 —— 消灭 panic 路径（3～5 天，机械替换，低风险）

按 §4 P0-4 的四类分别处理：锁中毒 → `map_err`；断言式 `expect` → 真正处理；
库里的 `panic!` → 返回 `Result`；`unwrap` 表达的不变式 → `ok_or_else` 或重构。
目标：生产代码 `unwrap_used` / `expect_used` / `panic` 三个 lint 全部 deny 且归零。

### 阶段 3 —— 控制流函数式化（1～2 周，逐文件推进）

优先顺序（按 §3.3 与 clippy 热点）：
`domain/download.rs` → `domain/source/local.rs` → `core/backup.rs` → `db/config.rs` → `rest/routes/image.rs`。
主要动作：`manual_let_else` 23 处、`map_unwrap_or` 116 处、`option_if_let_else` 76 处
直接吃 clippy 的自动修复（`cargo clippy --fix`），再手工处理索引循环与累积器。

### 阶段 4 —— 架构层去 OOP（2～4 周，最大收益也最大风险，建议单独立项）

1. 拆掉 3 个全局单例 → 引入只读的 `Arc<AppPaths>`，显式传参（P0-3）。
2. `DownloadManager` 拆成"不可变队列快照 + 纯迁移函数 + 单个 actor 持有可变状态"（P0-2）。
3. `Arc<dyn SourceFetcher>` 改为泛型或封闭 enum（P0-1），先从 `MangaService` /
   `ChapterService` 这两个最小服务对象试点，验证编译期开销可接受再铺开。
4. `PbReader`、`XmlWriter` 改成纯函数 / 不可变树（P1-5），这两个是独立模块，
   改动面小、收益直观，适合作为函数式重构的样板。

---

## 七、阶段 1–2 落地记录（分支 `refactor/guard-enhance`）

> 执行时间：2026-09-26　结果：`cargo fmt --check` / `clippy -D warnings` / `cargo test` 三道门全绿
> 改动：87 个文件（+1,732 / −1,423），其中约 1,500 行是 `cargo fmt` 的重排

### 阶段 1 —— 立门禁

| 动作 | 文件 | 说明 |
|------|------|------|
| 格式对齐 | `rustfmt.toml` | `edition = "2021"` → `"2024"`，与 workspace 一致；`cargo fmt --all` 吃掉 **399 处 diff** |
| 统一 lint | 根 `Cargo.toml` `[workspace.lints]` | `unsafe_code`、`rust_2018_idioms`、`unwrap_used`、`expect_used`、`panic`、`unreachable`、`todo`、`await_holding_lock` 全部 warn |
| lint 继承 | 9 个 `crates/*/Cargo.toml` | 各加 `[lints] workspace = true` |
| CI 门禁 | `.github/workflows/lint.yml`（新增） | 三个 job：`fmt` / `clippy (-D warnings)` / `test`，push 与 PR 触发，与出包流水线 `build.yml` 分离 |
| 测试豁免 | 10 个 crate 根 + 7 个 `tests/*.rs` | 加 `#![cfg_attr(test, allow(clippy::unwrap_used, …))]` —— 测试里 panic 就是断言，生产代码不受影响 |

刻意没开的：`cast_possible_truncation` / `cast_possible_wrap` / `cast_sign_loss`（182 处）留到阶段 3，
每一处都要先判断真实取值范围，属于语义改动而非机械替换。

### 阶段 2 —— 生产代码 panic 路径清零

clippy（`--lib --bins`，排除测试）实测 **29 处**（7 `unwrap` + 19 `expect` + 2 `panic!` + 1 `unreachable!`）→ **0 处**。
按性质分四类处理：

**① 真实错误路径 —— 改成传播或降级（24 处）**

| 位置 | 改法 |
|------|------|
| `db/row.rs:77` `panic!` | 删掉 `Row::get`（库里 panic 的 API）。唯一调用点 `domain/download.rs` 改用 `try_get` + `let … else { continue }` |
| `db-macros/lib.rs:20` `expect` | 派生宏改成返回 `syn::Error` → 编译错误能定位到字段，不再是 "proc macro panicked" |
| `core/config/mod.rs` ×2 | `RwLock` 中毒 → `unwrap_or_else(PoisonError::into_inner)`，读旧值继续跑 |
| `domain/download.rs` ×3、`source/sandbox.rs` ×3 | `Mutex` 中毒 → 同上；`Drop` 里尤其不能 panic（否则 JVM 变孤儿进程） |
| `domain/download.rs:240` | `queue.remove(pos).expect(...)` → `if let Some(job) = queue.remove(pos)` |
| `domain/download.rs:560` | 信号量关闭 → `map_err(...)?`，只让这一页下载失败 |
| `domain/{extension_store,koreader_sync,sync_yomi,source/sandbox}` ×4 | `ClientBuilder::build().expect("reqwest client")` → 新增 `domain/src/http.rs::build_client()`，TLS 后端失败时降级到默认客户端并告警 |
| `graphql/query.rs:1088` `unreachable!` | 返回 `async_graphql::Error`，只让这一个查询失败 |
| `server/lib.rs` ×4 `expect("build response")` | 抽出 `bytes_response()`，失败时返回 500 而不是 panic（panic 会让 axum 断流） |
| `android/lib.rs:170` | runtime 创建失败 → 返回错误码 `6`（与既有 4/5 同一套 JNI 约定），不再让 App 闪退 |

**② 按构造不可达 —— 保留 `expect` 但写明理由（1 处）**
`core/auth.rs` 抽出 `fn mac()`：`HmacSha256::new_from_slice` 的 `InvalidLength` 在 HMAC 上不可达
（RFC 2104 接受任意长度密钥）。保留 `expect` + 一句说明 + `#[allow(clippy::expect_used)]`，
而不是留一个没有上下文的 `unwrap()`。

**③ 非生产路径 —— 带说明放行（4 处）**
`db::test_support::db_lock()`（3 处 `expect`）：测试脚手架，拿不到锁文件说明环境已坏；
`server/build.rs`（1 处 `panic!`）：构建脚本里 panic 就是"构建失败"的正确表达，不进运行时。

**④ unsafe 边界 —— 补 `SAFETY:` 注释后放行（4 处）**
`android/lib.rs`（JNI `#[no_mangle]`，crate 级）、`server/main.rs:32`（Win32 单实例互斥体）、
`server/lib.rs::android_log`（liblog FFI，模块级，含两条 SAFETY 说明）。

**顺带清掉的**：`rust_2018_idioms` 的 16 处省略生命周期（`&FeedCtx` → `&FeedCtx<'_>` 等）、
`clippy::manual_filter`（`category/mod.rs:99` 的 `and_then(|n| if …)` → `filter(|n| …)`）。

### 门禁现状

| 门 | 命令 | 结果 |
|----|------|------|
| 格式 | `cargo fmt --all --check` | ✅ 0 diff |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 warning |
| 测试 | `cargo test --workspace` | ✅ 154 passed / 0 failed |
| pedantic + nursery | `-- -W clippy::pedantic -W clippy::nursery` | 2,642 条（阶段 3 的工作量） |

## 附录 A：命中规则对照表（rust-skills）

| 规则 ID | 命中程度 | 主要证据 |
|---------|---------|----------|
| `anti-type-erasure` | 高 | 26 处 `Arc<dyn …>` |
| `anti-unwrap-abuse` / `err-no-unwrap-prod` | 高 | 73 处 `unwrap` |
| `anti-expect-lazy` | 高 | 95 处 `expect`，部分文案无意义 |
| `anti-stringly-typed` | 高 | `DomainError` 6 个 `String` 变体 |
| `anti-index-over-iter` | 中 | 209 处下标、11 处索引循环 |
| `anti-clone-excessive` | 中 | 508 次 `.clone()`、73% struct 派生 `Clone` |
| `anti-over-abstraction` | 中 | `Arc<Vec<Arc<dyn TrackerService>>>` |
| `anti-panic-expected` | 中 | 4 处 `panic!` |
| `anti-lock-across-await` | 低（当前无跨 await 持锁） | `Arc<Mutex<…>>` ×9，需持续监控 |
| `lint-rustfmt-check` | 高 | 399 处 diff，CI 未接入 |
| `lint-workspace-lints` | 高 | 无 `[workspace.lints]` |
| `lint-clippy-nursery-selected` / `lint-pedantic-selective` | 高 | 2,664 条 pedantic+nursery |
| `lint-unsafe-doc` | 低 | 3 处 `unsafe` 缺 `SAFETY:` |
| `lint-missing-docs` | 中 | `missing_errors_doc` 342 |
| `num-*`（5 条） | 高 | 521 处未检查 cast |
| `api-must-use` | 中 | 只有 1 处 `#[must_use]`，350 个候选 |
| `api-common-traits` / `conv-*` | 中 | `Display`/`TryFrom`/`AsRef` 全为 0 |
| `err-thiserror-lib` | ✅ 达标 | 3 个 crate 已用 thiserror |
| `err-question-mark` | ✅ 达标 | 1,140 处 `?` |
| `pat-let-else` | ✅ 达标 | 127 处 |
| `unsafe-*`（7 条） | ✅ 基本达标 | 仅 3 处，集中在 FFI 边界 |

## 附录 B：复现命令

```bash
# 1) 格式门禁
cargo fmt --all --check            # 当前：399 处 diff

# 2) 默认 clippy（当前：1 warning）
cargo clippy --workspace --all-targets --message-format=short

# 3) pedantic + nursery（当前：2,664 warnings）
cargo clippy --workspace --all-targets -- -W clippy::pedantic -W clippy::nursery

# 4) 一键吃自动修复（建议先跑 fmt，再跑 clippy --fix，逐批 review）
cargo fmt --all && cargo clippy --fix --workspace --allow-dirty --allow-staged
```
