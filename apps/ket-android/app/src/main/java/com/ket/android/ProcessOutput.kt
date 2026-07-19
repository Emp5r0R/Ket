package com.ket.android

import java.io.IOException
import java.io.InputStream

internal fun consumeProcessOutput(input: InputStream, onLine: (String) -> Unit) {
    try {
        input.bufferedReader().useLines { lines -> lines.forEach(onLine) }
    } catch (_: IOException) {
        // Destroying a child process closes its output while this daemon thread may still be reading.
    }
}
