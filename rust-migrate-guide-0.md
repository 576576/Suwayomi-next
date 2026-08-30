以下是 Suwayomi-Server 核心迁移到 Rust 的完整可执行方案，可直接喂给 Agent 执行。

---

# Suwayomi-Server 核心迁移到 Rust —— 完整执行方案

## 一、现状分析

### 1.1 项目概述

Suwayomi-Server 是一个用 **Kotlin/JVM** 编写的免费开源漫画阅读服务器，核心能力是加载并运行 Mihon (Tachiyomi) 的 Android 扩展（APK 转 JAR），通过 GraphQL API 对外提供服务。

### 1.2 核心模块（基于源码结构）

| 模块              | 路径             | 职责                                                         |
| ----------------- | ---------------- | ------------------------------------------------------------ |
| **server**        | `/server`        | 应用入口（App.kt）、依赖注入（AppModule.kt）、GraphQL API 层、业务逻辑、数据持久化 |
| **AndroidCompat** | `/AndroidCompat` | Android 兼容层，提供 JVM 上运行 Android 扩展所需的 Android 框架类（android.jar） |
| **buildSrc**      | `/buildSrc`      | Gradle 构建配置                                              |
| **扩展加载器**    | 内置于 server    | 使用 URLClassLoader（计划迁移到 ChildFirstPathClassLoader）动态加载扩展 JAR |

### 1.3 技术债务与迁移动机

- **JVM 依赖**：需要 JRE 21+ 才能运行
- **内存占用**：JVM 基础开销较大
- **扩展加载机制**：当前 URLClassLoader 采用 Parent First 模型，与 Android 的 DexClassLoader 行为不一致，已计划重构
- **社区贡献门槛**：Kotlin/Java 开发者基数虽大，但项目希望探索更现代化的技术栈

---

## 二、目标架构

### 2.1 最终架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                        客户端 (WebUI / Sorayomi / 其他)          │
└─────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ (GraphQL / OPDS)
┌─────────────────────────────────────────────────────────────────┐
│                    Rust 服务端 (Axum / Actix-web)               │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              GraphQL API 层 (Async-graphql)             │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │          业务逻辑层 (Rust Core Library)                  │   │
│  │  • 漫画/章节/页面数据模型                                 │   │
│  │  • 库管理 (Library)                                      │   │
│  │  • 下载/更新/缓存逻辑                                    │   │
│  │  • 备份/恢复 (Mihon 兼容)                               │   │
│  │  • Tracker (MAL/AniList)                                │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │        扩展调用桥接层 (FFI → JVM 沙盒)                   │   │
│  │  • 通过 JNI 调用 JVM 加载 AndroidCompat + 扩展 JAR      │   │
│  │  • 或将扩展运行在独立 JVM 进程中，通过 IPC 通信          │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ (JNI / IPC)
┌─────────────────────────────────────────────────────────────────┐
│                JVM 沙盒 (扩展运行环境)                           │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              AndroidCompat (Android 框架兼容层)          │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │         扩展 JAR (从 Mihon APK 转换)                     │   │
│  │  • 通过 ChildFirstPathClassLoader 加载                   │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                    数据持久层 (SQLite / PostgreSQL)             │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 技术选型

| 组件               | 技术选型                 | 理由                                     |
| ------------------ | ------------------------ | ---------------------------------------- |
| **核心业务逻辑**   | Rust                     | 高性能、内存安全、无 GC 停顿             |
| **服务端框架**     | Axum 或 Actix-web        | 生态成熟，异步支持完善                   |
| **GraphQL**        | async-graphql            | Rust 生态最成熟的 GraphQL 实现           |
| **数据库**         | SQLx + SQLite/PostgreSQL | 异步、类型安全，保持与现有数据格式兼容   |
| **FFI/跨语言调用** | uniffi-rs                | Mozilla 出品，可生成 Kotlin 绑定，已验证 |
| **JVM 互操作**     | JNI 或 j4rs              | 在 Rust 中调用 Java 方法                 |
| **序列化**         | Serde                    | Rust 标准                                |
| **HTTP 客户端**    | reqwest                  | 异步、支持拦截器（替代 OkHttp）          |
| **日志**           | tracing                  | 结构化日志                               |

---

## 三、分阶段迁移计划（6 个 Phase）

### Phase 0：准备与调研（2 周）

**目标**：建立迁移基础，验证关键技术可行性。

**任务清单**：

- [ ] **Fork 代码仓库**，创建 `migration/rust-core` 分支
- [ ] **建立 Rust 开发环境**：安装 Rust、Cargo、clippy、rustfmt
- [ ] **定义核心数据模型**：在 Rust 中定义 `Manga`、`Chapter`、`Page`、`Category`、`LibraryEntry` 等结构体，与 Kotlin 版本保持字段一致
- [ ] **验证 uniffi-rs**：创建最小 Rust 库，导出简单函数（如 `add(a, b) -> i32`），在 Kotlin 中调用成功
- [ ] **验证 JNI 调用**：在 Rust 中通过 JNI 调用一个简单的 Java 静态方法
- [ ] **搭建项目骨架**：
  ```bash
  cargo new suwayomi-core --lib
  cargo new suwayomi-server --bin
  ```
- [ ] **编写迁移追踪文档**：记录每个 Kotlin 文件对应的 Rust 实现状态

**产出**：
- Rust 项目骨架
- uniffi-rs 集成 Demo（Rust → Kotlin 调用成功）
- JNI 调用 Demo（Rust → JVM 调用成功）
- 数据模型定义（Rust 版）

---

### Phase 1：核心数据模型与业务逻辑（4 周）

**目标**：将最核心、最独立的业务逻辑迁移到 Rust。

**迁移范围**（按优先级排序）：

| 优先级 | Kotlin 模块/类                        | Rust 对应模块        | 说明                           |
| ------ | ------------------------------------- | -------------------- | ------------------------------ |
| P0     | 数据模型 (Manga, Chapter, Page, etc.) | `src/models/`        | 无外部依赖，最安全             |
| P0     | 序列化/反序列化                       | `src/serialization/` | 使用 Serde，保持 JSON 格式兼容 |
| P1     | 库管理 (Library 增删改查)             | `src/library/`       | 依赖数据模型 + 数据库          |
| P1     | 分类管理 (Categories)                 | `src/category/`      | 依赖数据模型 + 数据库          |
| P2     | 备份/恢复 (Mihon 兼容)                | `src/backup/`        | 独立功能，可后置               |
| P2     | Tracker (MAL/AniList)                 | `src/tracker/`       | 独立功能，可后置               |

**任务清单**：

- [ ] **Week 1-2：数据模型**
  - 在 `src/models/` 中实现所有数据模型
  - 实现 `serde::Serialize` 和 `serde::Deserialize`
  - 编写单元测试，验证序列化输出与 Kotlin 版本一致
  - 使用 `uniffi-rs` 生成 Kotlin 绑定，确保 Kotlin 侧可以操作这些模型

- [ ] **Week 3-4：数据库层**
  - 使用 `sqlx` 实现数据库迁移（保持与 H2/PostgreSQL 兼容）
  - 实现 CRUD 操作
  - 编写集成测试（使用 SQLite 内存数据库）

**产出**：
- `suwayomi-core` crate 包含完整数据模型和数据库层
- 通过 `uniffi-rs` 生成 Kotlin 绑定
- 单元测试覆盖率 > 80%

---

### Phase 2：扩展运行沙盒（JVM 桥接层）（6 周）

**目标**：解决最大的技术难题——在 Rust 服务中运行 Android 扩展。

**架构决策**：采用 **独立 JVM 进程 + IPC 通信** 方案（而非嵌入 JVM），原因：
- 隔离性更好，JVM 崩溃不影响 Rust 主进程
- 可独立重启 JVM 沙盒
- 避免 JNI 的复杂内存管理

```
┌─────────────┐         IPC (HTTP/gRPC)          ┌─────────────┐
│  Rust 服务  │ ◄──────────────────────────────► │ JVM 沙盒   │
│  (主进程)   │                                   │ (子进程)   │
└─────────────┘                                   └─────────────┘
                                                          │
                                                          ▼
                                                   ┌─────────────┐
                                                   │ AndroidCompat│
                                                   │  + 扩展 JAR │
                                                   └─────────────┘
```

**任务清单**：

- [ ] **Week 1-2：提取 JVM 沙盒**
  - 从现有 Kotlin 代码中提取扩展加载和执行逻辑，封装为独立的 Java/Kotlin 模块
  - 模块暴露 HTTP 接口：`/fetch_manga`、`/fetch_chapters`、`/fetch_pages`、`/search`
  - 实现 `ChildFirstPathClassLoader` 替代 `URLClassLoader`

- [ ] **Week 3-4：Rust 侧 IPC 客户端**
  - 在 Rust 中实现 HTTP 客户端，调用 JVM 沙盒的接口
  - 定义请求/响应的数据结构（与 Phase 1 模型对齐）
  - 实现错误处理和重试逻辑

- [ ] **Week 5-6：集成测试与优化**
  - 端到端测试：Rust 服务 → JVM 沙盒 → 扩展 → 返回漫画数据
  - 性能测试：对比纯 JVM 方案的延迟差异
  - 实现沙盒进程的生命周期管理（启动、健康检查、优雅关闭）

**产出**：
- 独立的 JVM 沙盒模块（可单独运行）
- Rust 侧的 IPC 客户端
- 端到端集成测试通过

---

### Phase 3：GraphQL API 层（4 周）

**目标**：在 Rust 中实现与现有版本完全兼容的 GraphQL API。

**任务清单**：

- [ ] **Week 1-2：API 兼容性分析**
  - 使用 GraphQL introspection 导出当前 API Schema
  - 在 Rust 中使用 `async-graphql` 定义完全相同的 Schema
  - 实现所有 Query 和 Mutation 的 resolver

- [ ] **Week 3-4：集成业务逻辑**
  - 将 Phase 1 的业务逻辑挂接到 resolver 中
  - 扩展相关 resolver 通过 IPC 调用 Phase 2 的 JVM 沙盒
  - 实现 WebUI 静态文件托管（与现有版本一致）

**产出**：
- Rust 服务端可启动，GraphQL endpoint 可用
- 使用 `graphql-client` 或 Postman 验证 API 与 Kotlin 版本一致

---

### Phase 4：数据迁移与双轨运行（3 周）

**目标**：允许用户从 Kotlin 版本平滑迁移到 Rust 版本。

**任务清单**：

- [ ] **Week 1：数据迁移工具**
  - 实现从 H2/PostgreSQL 到 SQLite/PostgreSQL 的数据迁移
  - 支持读取现有的 `.db` 文件和备份文件

- [ ] **Week 2-3：双轨运行模式**
  - Rust 服务支持读取 Kotlin 版本的数据目录
  - 提供 `--migrate` 命令行参数，一键迁移
  - 编写迁移文档

**产出**：
- 数据迁移工具
- 用户迁移指南

---

### Phase 5：全量替换与发布（2 周）

**目标**：Rust 版本达到生产可用状态。

**任务清单**：

- [ ] **Week 1：性能优化与测试**
  - 性能基准测试（对比 Kotlin 版本）
  - 内存泄漏检测
  - 并发测试

- [ ] **Week 2：发布准备**
  - 跨平台构建（Windows、macOS、Linux）
  - 编写 Dockerfile
  - 更新 README 和文档
  - 发布 v3.0.0-alpha 版本

**产出**：
- 可发布的 Rust 版本
- Docker 镜像
- 完整的用户文档

---

### Phase 6：后续优化（长期）

- [ ] 探索直接加载 Android 扩展 APK（无需预转 JAR）
- [ ] 替代 JVM 沙盒，使用 Wasm 运行扩展（长期愿景）
- [ ] 支持 Android/iOS 原生（通过 uniffi-rs）
- [ ] 插件系统的 Rust 原生实现

---

## 四、关键技术实现细节

### 4.1 uniffi-rs 集成

在 `Cargo.toml` 中配置：

```toml
[lib]
crate-type = ["cdylib", "staticlib"]

[dependencies]
uniffi = { version = "0.28", features = ["build"] }

[build-dependencies]
uniffi = { version = "0.28", features = ["bindgen"] }
```

UDL 文件示例（`src/suwayomi_core.udl`）：

```idl
namespace suwayomi_core {
  Manga get_manga(u64 id);
  sequence<Manga> search_manga(string query);
};

dictionary Manga {
  u64 id;
  string title;
  string? author;
  string? artist;
  string? description;
  string? cover_url;
  string status;
};
```

构建后，在 Kotlin 中调用：

```kotlin
import uniffi.suwayomi_core.*

val manga = getManga(123)
println(manga.title)
```

### 4.2 JVM 沙盒 IPC 协议

定义 RESTful API（或 gRPC）：

| Endpoint                                       | Method | Request         | Response      |
| ---------------------------------------------- | ------ | --------------- | ------------- |
| `/extensions`                                  | GET    | -               | `Extension[]` |
| `/sources`                                     | GET    | -               | `Source[]`    |
| `/source/{sourceId}/manga`                     | GET    | `?query=&page=` | `Manga[]`     |
| `/source/{sourceId}/manga/{mangaId}`           | GET    | -               | `MangaDetail` |
| `/source/{sourceId}/manga/{mangaId}/chapters`  | GET    | -               | `Chapter[]`   |
| `/source/{sourceId}/chapter/{chapterId}/pages` | GET    | -               | `Page[]`      |

### 4.3 数据库兼容性

使用 `sqlx` 的 `sqlite` 和 `postgres` 特性，通过 feature flag 切换：

```toml
[dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "postgres"] }
```

迁移脚本放在 `migrations/` 目录，与 Kotlin 版本的 schema 保持一致。

---

## 五、风险与应对

| 风险               | 影响 | 应对措施                                                     |
| ------------------ | ---- | ------------------------------------------------------------ |
| **扩展兼容性**     | 高   | Phase 2 重点攻关；保留 JVM 沙盒方案作为兜底                  |
| **API 不一致**     | 高   | Phase 3 使用 Schema 比对工具（如 `graphql-inspector`）持续验证 |
| **性能不达预期**   | 中   | Phase 5 进行基准测试，定位瓶颈优化                           |
| **社区贡献下降**   | 中   | 保持 Kotlin 版本维护直至 Rust 版本稳定；提供详细的贡献指南   |
| **迁移工期超预期** | 中   | 每个 Phase 设置明确的 [Done] 条件，及时调整范围              |
| **数据迁移丢失**   | 高   | Phase 4 实现迁移前备份机制；提供回滚方案                     |

---

## 六、里程碑与检查点

| 里程碑               | 时间    | 验收标准                             |
| -------------------- | ------- | ------------------------------------ |
| **M0: 调研完成**     | Week 2  | uniffi-rs 和 JNI Demo 跑通           |
| **M1: 核心库完成**   | Week 6  | 数据模型 + 数据库层，单元测试通过    |
| **M2: 扩展沙盒完成** | Week 12 | Rust → JVM 沙盒 → 扩展，端到端跑通   |
| **M3: API 层完成**   | Week 16 | GraphQL API 与 Kotlin 版本 100% 兼容 |
| **M4: Alpha 发布**   | Week 21 | 首个可用的 Rust 版本发布             |

---

## 七、Agent 执行指令

将以上方案喂给 Agent 时，建议按以下方式分段执行：

### 执行顺序

1. **先执行 Phase 0**：验证 uniffi-rs 和 JNI 可行性。如果任一验证失败，调整方案。
2. **并行执行 Phase 1 + Phase 2 的准备工作**：数据模型可以独立于扩展沙盒开发。
3. **Phase 3 依赖 Phase 1 + Phase 2**：API 层需要两者的输出。
4. **Phase 4-6**：在核心功能稳定后进行。

### 每个 Phase 的执行模板

```
## Phase N 执行指令

### 子任务
- [ ] 子任务 1
- [ ] 子任务 2

### 验收标准
- [ ] 标准 1
- [ ] 标准 2

### 产物
- 文件路径 1
- 文件路径 2

### 阻塞问题
- 问题 1（如需外部决策）
```

### 推荐的 Agent 工作流

1. 每周执行一个 Sprint（对应上述 Phase 的子集）
2. 每个 Sprint 结束后运行完整的测试套件
3. 维护一个 `MIGRATION_STATUS.md` 文件，追踪每个 Kotlin 文件的迁移状态

---

## 八、参考资源

- Suwayomi-Server 仓库：https://github.com/Suwayomi/Suwayomi-Server
- Rust 重写讨论（Issue #1342）：https://github.com/Suwayomi/Suwayomi-Server/issues/1342
- uniffi-rs 官方文档：https://mozilla.github.io/uniffi-rs/