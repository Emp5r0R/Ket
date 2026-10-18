package com.ket.android.ui.metrics

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ThroughputTest {
    @Test
    fun `running totals become speeds per second`() {
        val history = listOf(
            TrafficSample(10_000, sent = 0, received = 0),
            TrafficSample(12_000, sent = 2_048, received = 8_192),
            TrafficSample(14_000, sent = 3_072, received = 8_192),
        )
        assertEquals(
            listOf(
                TrafficRate(12_000, sent = 1_024.0, received = 4_096.0),
                TrafficRate(14_000, sent = 512.0, received = 0.0),
            ),
            Throughput.rates(history),
        )
        assertEquals(TrafficRate(14_000, sent = 512.0, received = 0.0), Throughput.latest(history))
    }

    @Test
    fun `a total that started again counts as moved in that interval`() {
        val history = listOf(
            TrafficSample(0, sent = 90_000, received = 90_000),
            TrafficSample(2_000, sent = 1_000, received = 4_000),
        )
        assertEquals(TrafficRate(2_000, sent = 500.0, received = 2_000.0), Throughput.latest(history))
    }

    @Test
    fun `history skips repeated readings and keeps the most recent samples`() {
        var history = emptyList<TrafficSample>()
        assertNull(Throughput.latest(history))
        repeat(Throughput.HISTORY + 5) { index ->
            history = Throughput.append(history, TrafficSample(index * 2_000L, index.toLong(), index.toLong()))
        }
        assertEquals(Throughput.HISTORY, history.size)
        assertEquals(10_000L, history.first().atMillis)
        assertEquals(history, Throughput.append(history, history.last()))
        assertEquals(history, Throughput.append(history, history.last().copy(atMillis = 1)))
    }

    @Test
    fun `speeds are formatted per second`() {
        assertEquals("--", formatRate(null))
        assertEquals("512 B/s", formatRate(512.0))
        assertEquals("1.5 MB/s", formatRate(1_572_864.0))
    }
}
