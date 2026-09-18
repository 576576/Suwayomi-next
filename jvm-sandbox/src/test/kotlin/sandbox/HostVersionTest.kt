package sandbox

import eu.kanade.tachiyomi.AppInfo
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * `AppInfo` is the host-side contract extensions (extension-lib 1.3/1.5) read: the
 * version goes into their User-Agent, the MIME list tells them which image URLs
 * the server can serve.
 */
class HostVersionTest {

    @Test
    fun installedVersionIsReturnedAsIs() {
        installHostVersion("3064", "r3064")
        assertEquals(3064, AppInfo.getVersionCode())
        assertEquals("r3064", AppInfo.getVersionName())
    }

    @Test
    fun missingEnvFallsBackToZero() {
        installHostVersion(null, null)
        assertEquals(0, AppInfo.getVersionCode())
        assertEquals("0", AppInfo.getVersionName())
    }

    @Test
    fun unparsableCodeFallsBackWithoutDroppingTheName() {
        installHostVersion("v3064", "r3064")
        assertEquals(0, AppInfo.getVersionCode())
        assertEquals("r3064", AppInfo.getVersionName())
    }

    @Test
    fun supportedImageMimeTypesMatchWhatTheServerServes() {
        val mimes = AppInfo.getSupportedImageMimeTypes()
        assertTrue("image/jpeg" in mimes)
        assertTrue("image/png" in mimes)
    }
}
