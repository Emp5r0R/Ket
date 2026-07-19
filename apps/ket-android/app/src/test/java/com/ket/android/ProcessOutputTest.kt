package com.ket.android

import java.io.InterruptedIOException
import java.io.InputStream
import org.junit.Assert.assertEquals
import org.junit.Test

class ProcessOutputTest {
    @Test
    fun closedProcessStreamDoesNotEscapeReaderThread() {
        val input = object : InputStream() {
            private val bytes = "ready\n".toByteArray()
            private var offset = 0

            override fun read(): Int {
                if (offset < bytes.size) return bytes[offset++].toInt()
                throw InterruptedIOException("read interrupted by close() on another thread")
            }
        }
        val lines = mutableListOf<String>()

        consumeProcessOutput(input, lines::add)

        assertEquals(listOf("ready"), lines)
    }
}
