package com.ket.android

import java.net.Proxy
import java.net.URL
import javax.net.ssl.HttpsURLConnection

internal class RoutedInternetProbe(
    private val endpoints: List<String>,
    private val attempts: Int = 1,
    private val retryDelayMillis: Long = 0,
    private val pause: (Long) -> Unit = Thread::sleep,
    private val request: (String) -> Boolean,
) {
    fun verify(cancelled: () -> Boolean = { false }) {
        require(attempts > 0) { "Probe attempts must be positive" }
        repeat(attempts) { attempt ->
            for (endpoint in endpoints) {
                ensureEngineStartActive(cancelled)
                if (runCatching { request(endpoint) }.getOrDefault(false)) return
            }
            ensureEngineStartActive(cancelled)
            if (attempt + 1 < attempts) pause(retryDelayMillis)
        }
        throw IllegalStateException("The tunnel connected but carried no Internet traffic")
    }
}

internal object AndroidRoutedInternetProbe {
    private const val TIMEOUT_MILLIS = 5_000
    private const val RETRY_DELAY_MILLIS = 250L
    private val delegate = RoutedInternetProbe(
        endpoints = listOf(
            "https://connectivitycheck.gstatic.com/generate_204",
            "https://cp.cloudflare.com/generate_204",
        ),
        attempts = 2,
        retryDelayMillis = RETRY_DELAY_MILLIS,
        request = ::request,
    )

    fun verify(cancelled: () -> Boolean = { false }) = delegate.verify(cancelled)

    private fun request(endpoint: String): Boolean {
        val connection = URL(endpoint).openConnection(Proxy.NO_PROXY) as HttpsURLConnection
        return try {
            connection.connectTimeout = TIMEOUT_MILLIS
            connection.readTimeout = TIMEOUT_MILLIS
            connection.instanceFollowRedirects = false
            connection.useCaches = false
            connection.responseCode == HttpsURLConnection.HTTP_NO_CONTENT
        } finally {
            connection.disconnect()
        }
    }
}
