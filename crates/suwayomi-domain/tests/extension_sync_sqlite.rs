//! `sync_sources` 回归测试 —— SQLite 后端，不需要外部数据库或 JVM 沙盒。
//!
//! 覆盖的 bug：沙盒里已经加载、但 `extension` 表没有对应行的扩展，此前会被
//! `sync_sources` 直接跳过（当时的理由是"没有索引行时嵌套 SELECT 会撞 NOT NULL"）。
//! 后果分两种场景，都表现为**沙盒里有源、server 侧扩展列表却是空的**：
//!  - 桌面：手工丢进 `extensions/` 的 APK 没有仓库索引行；
//!  - Android：扩展来自系统 `PackageManager`，压根没有仓库索引可刷新。
//!
//! 同时覆盖配套行为：扩展从沙盒消失（Android 上走系统安装器卸载）后
//! `is_installed` 必须回写为未安装，但本地 APK 文件仍在的桌面行不能被误清。

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use suwayomi_core::db::Db;
use suwayomi_domain::extension_store::ExtensionStoreService;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const PKG: &str = "eu.kanade.tachiyomi.extension.all.nhentaicom";
const SOURCE_ID: i64 = 5591830863732393712;

const EXT_JSON: &str = r#"[{"pkgName":"eu.kanade.tachiyomi.extension.all.nhentaicom","name":"nhentai.com","lang":"all","versionName":"1.4.10","className":"keiyoushi.source.Generated","versionCode":14,"contentWarning":1,"sources":[{"id":5591830863732393712,"name":"nhentai.com","lang":"en"}]}]"#;
const SRC_JSON: &str = r#"[{"id":5591830863732393712,"name":"nhentai.com","lang":"en","extension":1}]"#;

/// 一个只回答 `/extensions` 与 `/sources` 的假沙盒；内容可随时改写，用来模拟
/// "扩展被卸载后 reload"。`Connection: close` 让 reqwest 每次新建连接，
/// accept 循环里一次只处理一条请求就够。
struct MockSandbox {
    base: String,
    state: Arc<Mutex<(String, String)>>,
}

impl MockSandbox {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock sandbox");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(Mutex::new((String::from("[]"), String::from("[]"))));
        let shared = state.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let shared = shared.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let body = {
                        let g = shared.lock().expect("mock state");
                        if path.starts_with("/extensions") {
                            g.0.clone()
                        } else if path.starts_with("/sources") {
                            g.1.clone()
                        } else {
                            String::from("{}")
                        }
                    };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self { base: format!("http://{addr}"), state }
    }

    fn set(&self, exts: &str, sources: &str) {
        *self.state.lock().expect("mock state") = (exts.to_string(), sources.to_string());
    }
}

fn tmp_root() -> PathBuf {
    let tmp = std::env::temp_dir().join(format!("ext-sync-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).expect("create tmp root");
    tmp
}

/// 独立临时目录的 service（不改进程环境变量，测试可并行）。
fn service_in(tmp: &Path, db: Db, base: String) -> ExtensionStoreService {
    let extensions = tmp.join("extensions");
    std::fs::create_dir_all(&extensions).expect("create extensions dir");
    ExtensionStoreService::with_cache_dir(db, Some(base), extensions, tmp.join("bin/extensions"), tmp.join("cache"))
}

/// 建表 —— 走与生产一致的迁移，顺带保证 SQL 在 SQLite 方言下可用。
async fn setup_db() -> Db {
    let db = Db::sqlite_in_memory().await.expect("open in-memory sqlite");
    db.migrate().await.expect("migrate");
    db
}

#[tokio::test]
async fn sandbox_extensions_without_index_rows_get_registered() {
    let db = setup_db().await;
    let tmp = tmp_root();
    let mock = MockSandbox::start().await;
    mock.set(EXT_JSON, SRC_JSON);
    let svc = service_in(&tmp, db.clone(), mock.base.clone());

    // 前置：表是空的（没有任何仓库索引行）
    let before: i64 = suwayomi_db::query_scalar("SELECT count(*) FROM suwayomi.extension")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(before, 0);

    let n = svc.sync_sources().await.expect("sync");
    assert_eq!(n, 1, "one source registered");

    // extension 行由沙盒元信息建立，且 version/nsfw 都得带过来
    let (name, lang, vc, cw, inst, cls): (String, String, i64, i32, bool, String) = suwayomi_db::query_as(
        "SELECT name, lang, version_code, content_warning, is_installed, class_name \
         FROM suwayomi.extension WHERE pkg_name = ?",
    )
    .bind(PKG)
    .fetch_one(db.pool())
    .await
    .expect("extension row");
    assert_eq!(name, "nhentai.com");
    assert_eq!(lang, "all");
    assert_eq!(vc, 14);
    assert_eq!(cw, 1, "manifest nsfw -> content_warning 1");
    assert!(inst);
    assert_eq!(cls, "keiyoushi.source.Generated");

    // source 行挂到该扩展上，并继承 content_warning
    let (sname, ext_id, s_cw): (String, i32, i32) =
        suwayomi_db::query_as("SELECT name, extension, content_warning FROM suwayomi.source WHERE id = ?")
            .bind(SOURCE_ID)
            .fetch_one(db.pool())
            .await
            .expect("source row");
    assert_eq!(sname, "nhentai.com");
    assert_eq!(ext_id, 1);
    assert_eq!(s_cw, 1);

    // 幂等：再同步一次不会插重复行
    let n2 = svc.sync_sources().await.expect("second sync");
    assert_eq!(n2, 1);
    let exts: i64 = suwayomi_db::query_scalar("SELECT count(*) FROM suwayomi.extension")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(exts, 1);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn vanished_extension_is_flagged_not_installed() {
    let db = setup_db().await;
    let tmp = tmp_root();
    let mock = MockSandbox::start().await;
    mock.set(EXT_JSON, SRC_JSON);
    let svc = service_in(&tmp, db.clone(), mock.base.clone());
    svc.sync_sources().await.expect("sync");

    // 扩展从系统里卸载 → 沙盒 reload 后不再报告它（Android 上收不到卸载回调，
    // 只能靠这次同步回写）
    mock.set("[]", "[]");
    let n = svc.sync_sources().await.expect("sync after uninstall");
    assert_eq!(n, 0);

    let inst: bool = suwayomi_db::query_scalar("SELECT is_installed FROM suwayomi.extension WHERE pkg_name = ?")
        .bind(PKG)
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert!(!inst, "扩展不在沙盒里了，必须回写为未安装");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn local_apk_on_disk_keeps_row_installed() {
    let db = setup_db().await;
    let tmp = tmp_root();
    let mock = MockSandbox::start().await;
    mock.set(EXT_JSON, SRC_JSON);
    let svc = service_in(&tmp, db.clone(), mock.base.clone());

    // 模拟"仓库索引行 + 本地已有 APK"（桌面场景）：即使沙盒这次没加载它，
    // 也不能把 is_installed 冲成 false —— 否则会和 upsert_index 的按文件判定
    // 来回打架
    let apk_name = "tachiyomi-all.nhentaicom-v1.4.10.apk";
    suwayomi_db::query(
        "INSERT INTO suwayomi.extension (apk_name, store_index_url, name, pkg_name, version_name, version_code, lang, content_warning, is_installed, class_name) \
         VALUES (?, 'http://127.0.0.1:1/repo/index.json', 'nhentai.com', ?, '1.4.10', 14, 'all', 1, TRUE, '')",
    )
    .bind(apk_name)
    .bind(PKG)
    .execute(db.pool())
    .await
    .expect("insert repo row");
    let mut f = std::fs::File::create(tmp.join("extensions").join(apk_name)).expect("create apk file");
    f.write_all(b"not a real apk").expect("write apk");

    // 沙盒这次一个扩展都没加载
    mock.set("[]", "[]");
    svc.sync_sources().await.expect("sync");

    let inst: bool = suwayomi_db::query_scalar("SELECT is_installed FROM suwayomi.extension WHERE pkg_name = ?")
        .bind(PKG)
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert!(inst, "APK 还在 extensions/ 里，保持已安装");

    let _ = std::fs::remove_dir_all(&tmp);
}
