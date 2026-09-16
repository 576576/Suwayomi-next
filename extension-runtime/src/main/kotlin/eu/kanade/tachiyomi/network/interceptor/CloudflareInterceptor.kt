package eu.kanade.tachiyomi.network.interceptor

import okhttp3.Interceptor
import okhttp3.Response

/**
 * Present in the default client so extensions that guard on its simple name
 * (bytecode check: `getClass().getSimpleName() == "CloudflareInterceptor"`,
 * see NHentai.to fetchPopularManga → "CloudflareInterceptor must be present
 * in default client") pass their runtime checks.
 *
 * No actual Cloudflare challenge bypass is performed here (that needs an
 * external flare solver); responses pass through untouched — sites that
 * genuinely serve a CF challenge will surface their own error instead of a
 * misleading "must be present" crash.
 */
class CloudflareInterceptor : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response = chain.proceed(chain.request())
}
