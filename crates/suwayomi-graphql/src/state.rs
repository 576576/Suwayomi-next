//! Shared GraphQL state — 装配层的应用状态，加上只有 GraphQL 才暴露的服务。
//!
//! 与 REST 共享的部分（`db` / `config` / `auth` / 各 service）**不再各存一份**：
//! 这里持有装配层建好的 [`AppState`] 并实现 [`Deref`]，所以 `state.db`、`state.manga`
//! 这类访问照旧写，但两边拿到的是同一份句柄。
//!
//! 为什么要 `Deref` 而不是平铺字段：这个结构上原本把 [`AppState`] 的 17 个字段
//! 抄了一遍，`new` 里也把同一批 service 又建了一遍。两套 `RuntimeConfig` 是其中最
//! 实际的后果——`reload_runtime_config` 只写得到 GraphQL 这份，设置页改了
//! `opdsCbzMimetype`，REST 取页读到的仍是 env 基线。字段改成 `app.<field>` 要动
//! 两百多处调用点，而 `Deref` 把「共享」这件事表达在类型上、调用点一个字不用改。
//!
//! 只属于自己的部分留在本结构上：KOReader / SyncYomi（只有 GraphQL 暴露）与备份
//! 恢复的内存状态。

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use suwayomi_api::AppState;
use suwayomi_core::config::ServerConfig;
use suwayomi_domain::koreader_sync::KoreaderSyncService;
use suwayomi_domain::sync_yomi::SyncYomiService;

use crate::mutation_b4::BackupRestoreStatus;

#[derive(Clone)]
pub struct GraphQLState {
    /// 装配层建好的应用状态；字段访问经 `Deref` 落到这里。
    app: AppState,
    /// env 基线。重算时以它为起点，避免上一次的覆盖被再叠一层。
    config_base: ServerConfig,
    /// KOReader progress sync.
    pub koreader: KoreaderSyncService,
    /// SyncYomi library sync.
    pub sync_yomi: SyncYomiService,
    /// In-memory results of finished backup restores (`restoreStatus(id:)`).
    backup_restores: Arc<tokio::sync::Mutex<HashMap<String, BackupRestoreStatus>>>,
}

impl Deref for GraphQLState {
    type Target = AppState;

    fn deref(&self) -> &AppState {
        &self.app
    }
}

impl GraphQLState {
    /// `config_base` 是 env 基线（未叠加持久化设置）：`reload_runtime_config` 每次
    /// 都从它重新算起，所以必须与 `app.config` 里那份「当前生效值」分开保存。
    pub fn new(app: AppState, config_base: ServerConfig) -> Self {
        // 两个服务读的是 `app.config` 这个句柄：设置改完 `replace` 进同一份，
        // 它们立刻能看到新值，不必重启。
        let koreader = KoreaderSyncService::new(app.db.clone(), app.config.clone());
        let sync_yomi = SyncYomiService::new(app.db.clone(), app.config.clone());
        Self {
            app,
            config_base,
            koreader,
            sync_yomi,
            backup_restores: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn set_backup_restore_status(&self, id: &str, status: BackupRestoreStatus) {
        self.backup_restores.lock().await.insert(id.to_string(), status);
    }

    pub async fn get_backup_restore_status(&self, id: &str) -> Option<BackupRestoreStatus> {
        self.backup_restores.lock().await.get(id).cloned()
    }

    /// 当前有效配置的快照（env 基线 + 持久化 blob）。
    pub fn config_snapshot(&self) -> ServerConfig {
        self.config.snapshot()
    }

    /// `global_meta` 里 `setSettings` 写下的 settings blob。
    async fn settings_blob(&self) -> Option<serde_json::Value> {
        use suwayomi_domain::sql::bind_placeholders;
        let sql = bind_placeholders("SELECT value FROM global_meta WHERE meta_key = ?");
        let row = suwayomi_db::query(&sql).bind("settings").fetch_optional(self.db.pool()).await.ok()??;
        let value = row.try_get::<String, _>("value").ok()?;
        serde_json::from_str::<serde_json::Value>(&value).ok()
    }

    /// 重算运行时配置：env 基线上盖一层持久化 blob，写进共享的 `config` 句柄。
    ///
    /// 启动时调用一次（让上次保存的设置生效），`setSettings` 之后再调用一次
    /// （让改动立刻生效）。不刷新的话 `config` 只是 env 基线，设置页改了
    /// KOReader 冲突策略 / SyncYomi 开关，运行时读到的还是 `ServerConfig` 默认值。
    ///
    /// 写的是 [`AppState`] 那份句柄，所以 REST / OPDS 也立刻看到新值。
    pub async fn reload_runtime_config(&self) {
        let mut config = self.config_base.clone();
        if let Some(blob) = self.settings_blob().await {
            config.apply_settings_blob(&blob);
        }
        self.config.replace(config);
    }

    /// `settings` 查询与 `setSettings` 返回值共用的装配：`ServerConfig` 上盖一层
    /// `global_meta` 里持久化的 blob（`setSettings` 写的）覆盖。
    ///
    /// blob 里 `ServerConfig` 不持有的那批字段（下载 / OPDS / FlareSolverr 等）
    /// 只有 `apply_overrides` 认识，所以这里仍要读一次 blob。
    pub async fn effective_settings(&self) -> crate::settings::SettingsType {
        let mut settings = crate::settings::SettingsType::from_config(&self.config.snapshot());
        if let Some(blob) = self.settings_blob().await {
            settings.apply_overrides(&blob);
        }
        settings
    }
}
