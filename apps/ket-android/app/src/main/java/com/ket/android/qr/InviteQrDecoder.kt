package com.ket.android.qr

import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.NotFoundException
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import java.nio.ByteBuffer

/**
 * Bounded QR decoding for invites (adapted from Abyssal's InviteQrDecoder, ZXing only).
 * Decoded text is data: callers must still verify it as an invite before use.
 */
internal object InviteQrDecoder {
    const val MAX_TEXT = 2_048
    const val MAX_SIDE = 1_280

    /** Decodes an 8-bit luminance raster of [width] x [height]; null when no QR code is found. */
    fun decodeLuminance(bytes: ByteArray, width: Int, height: Int): String? {
        require(width in 1..MAX_SIDE && height in 1..MAX_SIDE) { "QR raster is too large" }
        require(bytes.size == width * height) { "QR raster size does not match its dimensions" }
        val reader = QRCodeReader()
        return try {
            val bitmap = BinaryBitmap(HybridBinarizer(PlanarYUVLuminanceSource(bytes, width, height, 0, 0, width, height, false)))
            val result = try {
                reader.decode(bitmap)
            } catch (_: NotFoundException) {
                // Dense, perfectly aligned generator output can miss ordinary detection.
                reader.decode(bitmap, mapOf(DecodeHintType.PURE_BARCODE to true))
            }
            try {
                result.text?.takeIf { it.length in 1..MAX_TEXT }
            } finally {
                result.rawBytes?.fill(0)
            }
        } catch (_: ReaderException) {
            null
        } finally {
            reader.reset()
        }
    }

    /** Copies the luminance (Y) plane of a camera frame, honouring row and pixel strides. */
    fun copyPlane(buffer: ByteBuffer, width: Int, height: Int, rowStride: Int, pixelStride: Int): ByteArray {
        require(width in 1..MAX_SIDE && height in 1..MAX_SIDE && rowStride > 0 && pixelStride > 0)
        require((width - 1L) * pixelStride < rowStride)
        val view = buffer.duplicate()
        val start = view.position()
        val last = start.toLong() + (height - 1L) * rowStride + (width - 1L) * pixelStride
        require(last < view.limit()) { "Camera frame is truncated" }
        return ByteArray(width * height) { index ->
            view.get(start + (index / width) * rowStride + (index % width) * pixelStride)
        }
    }

    /** Converts ARGB pixels to luminance (ITU-R BT.601), compositing transparency onto white. */
    fun luminanceFromArgb(pixels: IntArray, width: Int, height: Int): ByteArray {
        require(width in 1..MAX_SIDE && height in 1..MAX_SIDE && pixels.size == width * height)
        return ByteArray(pixels.size) { index ->
            val pixel = pixels[index]
            val alpha = (pixel ushr 24) and 0xff
            val red = (pixel shr 16) and 0xff
            val green = (pixel shr 8) and 0xff
            val blue = pixel and 0xff
            val luminance = (red * 299 + green * 587 + blue * 114) / 1_000
            ((luminance * alpha + 255 * (255 - alpha)) / 255).toByte()
        }
    }
}
