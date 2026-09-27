//! Injekt bootstrap（Android 版）：与桌面 ext-runtime 的同名函数做同一件事，区别在依赖
//! 实例直接用**真的** `Application` / `Context`（系统已经给了我们），所以扩展拿到的偏好
//! 设置会真正落盘 —— 这比桌面沙盒更好，不是偏差。

package sandbox

import android.app.Application
import android.content.Context
import eu.kanade.tachiyomi.network.NetworkHelper
import kotlinx.serialization.json.Json
import org.koin.core.context.startKoin
import org.koin.dsl.module
import uy.kohesive.injekt.api.InjektScope
import uy.kohesive.injekt.api.KoinRegistrar

private var started = false

/**
 * 幂等：重复调用只生效一次（扩展 reload 时不需要重建 Koin 容器）。
 */
fun setupInjekt(app: Application) {
    if (started) return
    started = true
    val m = module {
        single { NetworkHelper() }
        single<Application> { app }
        single<Context> { app }
        // 扩展（Mihon lib）经 injekt 取它们的 JSON 编解码器
        single {
            Json {
                ignoreUnknownKeys = true
                coerceInputValues = true
                explicitNulls = false
            }
        }
    }
    startKoin { modules(m) }
    uy.kohesive.injekt.Injekt = InjektScope(KoinRegistrar())
}
