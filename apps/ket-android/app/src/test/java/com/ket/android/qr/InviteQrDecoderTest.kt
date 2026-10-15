package com.ket.android.qr

import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.ByteArrayInputStream
import java.nio.ByteBuffer

class InviteQrDecoderTest {
    private val invite = "ket:invite:" + "Ab0_-".repeat(50)

    private fun rasterFor(text: String, side: Int): ByteArray {
        val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, side, side, mapOf(EncodeHintType.MARGIN to 4))
        return ByteArray(side * side) { index -> if (matrix[index % side, index / side]) 0 else 0xff.toByte() }
    }

    @Test
    fun `invite QR codes round trip through luminance and ARGB rasters`() {
        val side = 480
        val luminance = rasterFor(invite, side)
        assertEquals(invite, InviteQrDecoder.decodeLuminance(luminance, side, side))

        val argb = IntArray(side * side) { index -> if (luminance[index] == 0.toByte()) 0xff000000.toInt() else 0xffffffff.toInt() }
        assertEquals(invite, InviteQrDecoder.decodeLuminance(InviteQrDecoder.luminanceFromArgb(argb, side, side), side, side))

        // Transparent background (alpha 0) composites onto white instead of black.
        val transparent = IntArray(side * side) { index -> if (luminance[index] == 0.toByte()) 0xff000000.toInt() else 0x00000000 }
        assertEquals(invite, InviteQrDecoder.decodeLuminance(InviteQrDecoder.luminanceFromArgb(transparent, side, side), side, side))

        assertNull(InviteQrDecoder.decodeLuminance(ByteArray(side * side) { 0x7f }, side, side))
    }

    @Test
    fun `rasters and camera planes are bounded and stride-aware`() {
        assertThrows(IllegalArgumentException::class.java) { InviteQrDecoder.decodeLuminance(ByteArray(4), 2, 3) }
        assertThrows(IllegalArgumentException::class.java) {
            InviteQrDecoder.decodeLuminance(ByteArray(1_281), 1_281, 1)
        }
        // 3x2 luminance with a row stride of 4 and pixel stride of 1.
        val plane = ByteBuffer.wrap(byteArrayOf(1, 2, 3, 99, 4, 5, 6))
        assertArrayEquals(byteArrayOf(1, 2, 3, 4, 5, 6), InviteQrDecoder.copyPlane(plane, 3, 2, 4, 1))
        assertThrows(IllegalArgumentException::class.java) {
            InviteQrDecoder.copyPlane(ByteBuffer.wrap(ByteArray(5)), 3, 2, 4, 1)
        }
    }

    @Test
    fun `selected images are size-limited and sampled down to the decoder bound`() {
        assertEquals(10, QrImageReader.readBounded(ByteArrayInputStream(ByteArray(10))).size)
        assertThrows(IllegalArgumentException::class.java) {
            QrImageReader.readBounded(ByteArrayInputStream(ByteArray(QrImageReader.MAX_IMAGE_BYTES + 1)))
        }
        assertEquals(1, QrImageReader.sampleSize(1_280, 720))
        assertEquals(2, QrImageReader.sampleSize(1_281, 720))
        assertEquals(4, QrImageReader.sampleSize(4_096, 4_096))
    }
}
