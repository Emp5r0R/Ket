package com.ket.android.share

/** A share the native proxy started. Its password is a secret: [toString] leaves it out. */
data class StartedShare(val handle: Long, val port: Int, val username: String, val password: String) {
    override fun toString(): String = "StartedShare(handle=$handle, port=$port, username=$username, password=[REDACTED])"
}

sealed interface ShareStart {
    data class Started(val share: StartedShare) : ShareStart
    data class Refused(val code: String) : ShareStart
}

data class ShareCounts(val devices: Int, val connections: Int, val refusedSignIns: Long)

/** The proxy that shares the connection, run by `crates/ket-share-android`. */
interface ShareEngine {
    fun start(address: String): ShareStart

    /** The VPN network's handle (`Network.getNetworkHandle()`), or 0 while there is none. */
    fun setNetwork(handle: Long, network: Long)

    /** Null once the share has stopped. */
    fun counts(handle: Long): ShareCounts?

    fun stop(handle: Long)
}

/** Reads the rows `NativeShare` returns; kept apart so it is tested without the library. */
internal object ShareRows {
    fun start(row: Array<String?>?): ShareStart {
        if (row == null) return ShareStart.Refused("native_failure")
        return when (row.firstOrNull()) {
            "ok" -> {
                val handle = row.getOrNull(1)?.toLongOrNull()
                val port = row.getOrNull(2)?.toIntOrNull()
                val username = row.getOrNull(3)
                val password = row.getOrNull(4)
                if (handle == null || port == null || username == null || password == null || row.size != 5) {
                    ShareStart.Refused("native_failure")
                } else {
                    ShareStart.Started(StartedShare(handle, port, username, password))
                }
            }
            "error" -> ShareStart.Refused(row.getOrNull(1) ?: "native_failure")
            else -> ShareStart.Refused("native_failure")
        }
    }

    fun counts(values: LongArray?): ShareCounts? =
        values?.takeIf { it.size == 3 }?.let { ShareCounts(it[0].toInt(), it[1].toInt(), it[2]) }
}

object NativeShareEngine : ShareEngine {
    override fun start(address: String): ShareStart = ShareRows.start(NativeShare.start(address))

    override fun setNetwork(handle: Long, network: Long) {
        NativeShare.setNetwork(handle, network)
    }

    override fun counts(handle: Long): ShareCounts? = ShareRows.counts(NativeShare.stats(handle))

    override fun stop(handle: Long) = NativeShare.stop(handle)
}

/** JNI entry points of `libket_share_jni.so`; see `crates/ket-share-android/src/lib.rs`. */
internal object NativeShare {
    init {
        System.loadLibrary("ket_share_jni")
    }

    @JvmStatic external fun start(address: String): Array<String?>?

    @JvmStatic external fun setNetwork(handle: Long, network: Long): Boolean

    @JvmStatic external fun stats(handle: Long): LongArray?

    @JvmStatic external fun stop(handle: Long)
}
