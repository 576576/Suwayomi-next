package eu.kanade.tachiyomi.network

/*
 * Copyright (C) Contributors to the Suwayomi project
 *
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import eu.kanade.tachiyomi.network.interceptor.UncaughtExceptionInterceptor
import eu.kanade.tachiyomi.network.interceptor.UserAgentInterceptor
import io.github.oshai.kotlinlogging.KotlinLogging
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.Cache
import okhttp3.OkHttpClient
import okhttp3.logging.HttpLoggingInterceptor
import java.net.CookieHandler
import java.net.CookieManager
import java.net.CookiePolicy
import java.net.InetSocketAddress
import java.net.Proxy
import java.nio.file.Files
import java.util.concurrent.TimeUnit

class NetworkHelper() {
    //    private val preferences: PreferencesHelper by injectLazy()

//    private val cacheDir = File(context.cacheDir, "network_cache")

//    private val cacheSize = 5L * 1024 * 1024 // 5 MiB

    // Tachidesk -->
    val cookieStore = PersistentCookieStore()

    init {
        CookieHandler.setDefault(
            CookieManager(cookieStore, CookiePolicy.ACCEPT_ALL),
        )
    }
    // Tachidesk <--

    private val userAgent =
        MutableStateFlow(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) " +
                "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
    val userAgentFlow = userAgent.asStateFlow()

    fun defaultUserAgentProvider(): String = userAgent.value


    private val baseClientBuilder: OkHttpClient.Builder
        get() {
            val builder =
                OkHttpClient
                    .Builder()
                    .cookieJar(PersistentCookieJar(cookieStore))
                    .connectTimeout(30, TimeUnit.SECONDS)
                    .readTimeout(30, TimeUnit.SECONDS)
                    .callTimeout(2, TimeUnit.MINUTES)
                    .cache(
                        Cache(
                            directory = Files.createTempDirectory("tachidesk_network_cache").toFile(),
                            maxSize = 5L * 1024 * 1024, // 5 MiB
                        ),
                    ).addInterceptor(UncaughtExceptionInterceptor())
                    .addInterceptor(UserAgentInterceptor(::defaultUserAgentProvider))

            // Optional outbound proxy (e.g. Clash) for reaching geo-blocked sources.
            // Format: SUWAYOMI_SANDBOX_PROXY=host:port
            val proxyEnv = System.getenv("SUWAYOMI_SANDBOX_PROXY")
            if (!proxyEnv.isNullOrBlank()) {
                val parts = proxyEnv.split(":")
                val host = parts.getOrNull(0)?.takeIf { it.isNotBlank() }
                val port = parts.getOrNull(1)?.toIntOrNull()
                if (host != null && port != null) {
                    builder.proxy(Proxy(Proxy.Type.HTTP, InetSocketAddress(host, port)))
                }
            }

            // if (preferences.verboseLogging().get()) {
            val httpLoggingInterceptor =
                HttpLoggingInterceptor(
                    object : HttpLoggingInterceptor.Logger {
                        val logger = KotlinLogging.logger { }

                        override fun log(message: String) {
                            logger.debug { message }
                        }
                    },
                ).apply {
                    level = HttpLoggingInterceptor.Level.BASIC
                }
            builder.addNetworkInterceptor(httpLoggingInterceptor)
            // }

            // when (preferences.dohProvider().get()) {
            //     PREF_DOH_CLOUDFLARE -> builder.dohCloudflare()
            //     PREF_DOH_GOOGLE -> builder.dohGoogle()
            //     PREF_DOH_ADGUARD -> builder.dohAdGuard()
            //     PREF_DOH_QUAD9 -> builder.dohQuad9()
            //     PREF_DOH_ALIDNS -> builder.dohAliDNS()
            //     PREF_DOH_DNSPOD -> builder.dohDNSPod()
            //     PREF_DOH_360 -> builder.doh360()
            //     PREF_DOH_QUAD101 -> builder.dohQuad101()
            //     PREF_DOH_MULLVAD -> builder.dohMullvad()
            //     PREF_DOH_CONTROLD -> builder.dohControlD()
            //     PREF_DOH_NJALLA -> builder.dohNajalla()
            //     PREF_DOH_SHECAN -> builder.dohShecan()
            // }

            return builder
        }

//    val client by lazy { baseClientBuilder.cache(Cache(cacheDir, cacheSize)).build() }
    val client by lazy { baseClientBuilder.build() }

    @Deprecated("The regular client handles Cloudflare by default")
    @Suppress("UNUSED")
    val cloudflareClient by lazy { client }
}
