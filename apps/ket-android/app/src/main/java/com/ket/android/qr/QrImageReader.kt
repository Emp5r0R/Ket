package com.ket.android.qr

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import java.io.ByteArrayOutputStream
import java.io.InputStream

/** Reads an invite QR code from a user-selected local image. Nothing is uploaded or retained. */
internal object QrImageReader {
    const val MAX_IMAGE_BYTES = 8 * 1024 * 1024
    const val MAX_SOURCE_SIDE = 4_096

    /** Returns the decoded QR text, or null when the image holds no readable QR code. */
    fun decode(context: Context, uri: Uri): String? {
        val bytes = context.contentResolver.openInputStream(uri)?.use(::readBounded) ?: return null
        try {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            require(bounds.outWidth in 1..MAX_SOURCE_SIDE && bounds.outHeight in 1..MAX_SOURCE_SIDE) {
                "Image dimensions are not supported"
            }
            val options = BitmapFactory.Options().apply {
                inSampleSize = sampleSize(bounds.outWidth, bounds.outHeight)
                inPreferredConfig = Bitmap.Config.ARGB_8888
            }
            val bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options) ?: return null
            try {
                val pixels = IntArray(bitmap.width * bitmap.height)
                bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
                val luminance = InviteQrDecoder.luminanceFromArgb(pixels, bitmap.width, bitmap.height)
                pixels.fill(0)
                return try {
                    InviteQrDecoder.decodeLuminance(luminance, bitmap.width, bitmap.height)
                } finally {
                    luminance.fill(0)
                }
            } finally {
                bitmap.recycle()
            }
        } finally {
            bytes.fill(0)
        }
    }

    /** Reads at most [MAX_IMAGE_BYTES]; larger inputs are rejected rather than truncated. */
    fun readBounded(input: InputStream): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
        while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            require(output.size() + read <= MAX_IMAGE_BYTES) { "Image is too large" }
            output.write(buffer, 0, read)
        }
        return output.toByteArray()
    }

    /** The power-of-two sample size that brings the longest side within [InviteQrDecoder.MAX_SIDE]. */
    fun sampleSize(width: Int, height: Int): Int {
        var sample = 1
        while (maxOf(width, height) / sample > InviteQrDecoder.MAX_SIDE) sample *= 2
        return sample
    }
}
