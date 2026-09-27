//! 进程内各根目录的显式句柄。
//!
//! 替代原先三处进程级单例（`CACHE_ROOT_OVERRIDE` / `LOCAL_ROOT_OVERRIDE` /
//! `DOWNLOADS_ROOT_OVERRIDE`）与对应的 `set_*_root()` setter：路径在启动时解析
//! 一次，之后**经构造参数注入**到需要它的服务，函数不再隐式依赖"之前有没有人
//! 调过 setter"。同一进程内可以并存多套路径（测试就是两套），不需要环境变量。

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// 进程内各根目录。
///
/// * `data` / `cache` —— 构造后不再变化，只读。
/// * `downloads` / `local_sources` —— 由 `downloadsPath` / `localSourcePath` 设置
///   驱动。WebUI 保存设置后这两项目标要**立即生效**（不重启进程），所以它们可以
///   整体替换；`Arc<RwLock<..>>` 克隆共享同一份，替换对所有持有者可见。
#[derive(Clone, Debug)]
pub struct AppPaths(Arc<RwLock<PathsInner>>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct PathsInner {
    data: PathBuf,
    cache: PathBuf,
    downloads: PathBuf,
    local_sources: PathBuf,
}

/// 本地图源根的环境变量名（托盘 spawn server 时设置）。
pub const LOCAL_SOURCE_DIR_ENV: &str = "SUWAYOMI_LOCAL_SOURCE_DIR";

/// 没有 `localSourcePath` 设置时本地图源根落在哪。
///
/// 解析顺序：`SUWAYOMI_LOCAL_SOURCE_DIR` env（托盘 spawn server 时 cwd=data，
/// 默认会解析成 `data/data/local`）→ exe `bin/` 布局的发布根 `data/local` →
/// `cwd/data/local`。与"数据目录在设置里被改过"无关 —— 数据目录只影响
/// `downloads` / `backups` 的默认落点，本地图源要有自己的一条环境变量通路。
fn default_local_source_root() -> PathBuf {
    if let Ok(dir) = std::env::var(LOCAL_SOURCE_DIR_ENV)
        && !dir.is_empty()
    {
        return PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.file_name().is_some_and(|n| n == "bin")
        && let Some(base) = dir.parent()
    {
        return base.join("data").join("local");
    }
    std::env::current_dir().unwrap_or_default().join("data").join("local")
}

impl AppPaths {
    /// 以数据目录 / 缓存根为基准构造，两个可替换根先落各自的默认值：
    /// `downloads` = `<data>/downloads`，`local_sources` = [`default_local_source_root`]。
    ///
    /// 缓存根必须由调用方显式给出 —— Android 宿主没有环境变量可用，
    /// 只能把 `<data>/cache` 直接传进来（见 `ServerOptions::cache_dir`）。
    pub fn new(data: PathBuf, cache: PathBuf) -> Self {
        let inner =
            PathsInner { downloads: data.join("downloads"), local_sources: default_local_source_root(), data, cache };
        Self(Arc::new(RwLock::new(inner)))
    }

    /// 锁中毒只说明"某个持有写锁的线程 panic 过"，路径值本身仍然完整；读路径不该
    /// 把一次查询升级成进程退出（与 `RuntimeConfig::snapshot` 同一口径）。
    fn read(&self) -> std::sync::RwLockReadGuard<'_, PathsInner> {
        self.0.read().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, PathsInner> {
        self.0.write().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 用户数据根：`backups/` 与自动备份的默认落点。
    pub fn data(&self) -> PathBuf {
        self.read().data.clone()
    }

    /// 统一缓存根：扩展图标 / 仓库索引 / 缩略图 / 图片代理 / 本地封面。
    pub fn cache(&self) -> PathBuf {
        self.read().cache.clone()
    }

    /// 下载根（`downloadsPath` 设置生效后的值）。
    pub fn downloads(&self) -> PathBuf {
        self.read().downloads.clone()
    }

    /// 本地图源根（`localSourcePath` 设置生效后的值）。
    pub fn local_sources(&self) -> PathBuf {
        self.read().local_sources.clone()
    }

    /// 覆盖下载根。`None` / 空路径回到默认的 `<data>/downloads`。
    pub fn set_downloads(&self, path: Option<PathBuf>) {
        let fallback = self.read().data.join("downloads");
        self.write().downloads = path.filter(|p| !p.as_os_str().is_empty()).unwrap_or(fallback);
    }

    /// 覆盖本地图源根。`None` / 空路径回到默认的 [`default_local_source_root`]
    /// （env → 发布布局 → `cwd/data/local`），与设置项被清空前的行为一致。
    pub fn set_local_sources(&self, path: Option<PathBuf>) {
        let fallback = default_local_source_root();
        self.write().local_sources = path.filter(|p| !p.as_os_str().is_empty()).unwrap_or(fallback);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn defaults_derive_from_data_dir() {
        let paths = AppPaths::new(PathBuf::from("E:/suwayomi"), PathBuf::from("E:/suwayomi/cache"));
        assert_eq!(paths.data(), PathBuf::from("E:/suwayomi"));
        assert_eq!(paths.cache(), PathBuf::from("E:/suwayomi/cache"));
        assert_eq!(paths.downloads(), PathBuf::from("E:/suwayomi/downloads"));
        // 本地图源默认不跟数据目录绑：走 env / 发布布局 / cwd 那条通路
        assert!(
            paths.local_sources().ends_with(Path::new("data").join("local")),
            "{}",
            paths.local_sources().display()
        );
    }

    #[test]
    fn overrides_are_visible_through_every_clone() {
        let paths = AppPaths::new(PathBuf::from("E:/suwayomi"), PathBuf::from("E:/cache"));
        let clone = paths.clone();
        paths.set_downloads(Some(PathBuf::from("F:/cbz")));
        paths.set_local_sources(Some(PathBuf::from("F:/local")));
        assert_eq!(clone.downloads(), PathBuf::from("F:/cbz"));
        assert_eq!(clone.local_sources(), PathBuf::from("F:/local"));
    }

    #[test]
    fn empty_or_missing_override_falls_back_to_default() {
        let paths = AppPaths::new(PathBuf::from("E:/suwayomi"), PathBuf::from("E:/cache"));
        paths.set_downloads(Some(PathBuf::from("F:/cbz")));
        paths.set_downloads(None);
        assert_eq!(paths.downloads(), PathBuf::from("E:/suwayomi/downloads"));
        // 空路径与 `None` 等价（WebUI 清空设置项后回到默认位置）
        paths.set_local_sources(Some(PathBuf::from("F:/local")));
        paths.set_local_sources(Some(PathBuf::from("")));
        assert_eq!(paths.local_sources(), default_local_source_root());
    }
}
