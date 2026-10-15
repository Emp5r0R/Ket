package com.ket.android.invite

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

private const val MAJOR_UINT = 0
private const val MAJOR_BYTES = 2
private const val MAJOR_TEXT = 3
private const val MAJOR_ARRAY = 4
private const val NULL = 0xf6

/**
 * Minimal deterministic CBOR decoder: unsigned integers, byte and text strings, arrays,
 * and `null`, each only in its shortest definite-length form. Mirrors `ket-invite` codec.rs.
 */
internal class CborDecoder(private val input: ByteArray) {
    private var offset = 0

    fun finish() = ensure(offset == input.size, InviteFailure.Invalid)

    fun array(expected: Int) = ensure(arrayLength() == expected.toULong(), InviteFailure.Invalid)

    fun arrayLength(): ULong = length(MAJOR_ARRAY)

    fun uint(): ULong {
        val head = byte()
        ensure(head ushr 5 == MAJOR_UINT, InviteFailure.Invalid)
        return argument(head and 0x1f)
    }

    fun optionalUint(): ULong? {
        if (offset < input.size && (input[offset].toInt() and 0xff) == NULL) {
            offset += 1
            return null
        }
        return uint()
    }

    fun bytes(max: Int): ByteArray = take(length(MAJOR_BYTES), max)

    fun text(max: Int): String {
        val bytes = take(length(MAJOR_TEXT), max)
        return try {
            StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(bytes))
                .toString()
        } catch (_: CharacterCodingException) {
            fail(InviteFailure.Invalid)
        }
    }

    private fun take(length: ULong, max: Int): ByteArray {
        ensure(length <= max.toULong(), InviteFailure.TooLarge)
        val size = length.toInt()
        ensure(size <= input.size - offset, InviteFailure.Invalid)
        return input.copyOfRange(offset, offset + size).also { offset += size }
    }

    private fun length(major: Int): ULong {
        val head = byte()
        ensure(head ushr 5 == major, InviteFailure.Invalid)
        return argument(head and 0x1f)
    }

    /** Decodes an argument and rejects any encoding longer than the shortest one. */
    private fun argument(additional: Int): ULong {
        val (value, minimum) = when (additional) {
            in 0..23 -> return additional.toULong()
            24 -> fixed(1) to 24UL
            25 -> fixed(2) to 0x100UL
            26 -> fixed(4) to 0x1_0000UL
            27 -> fixed(8) to 0x1_0000_0000UL
            else -> fail(InviteFailure.Invalid)
        }
        ensure(value >= minimum, InviteFailure.Invalid)
        return value
    }

    private fun byte(): Int {
        ensure(offset < input.size, InviteFailure.Invalid)
        return input[offset++].toInt() and 0xff
    }

    private fun fixed(width: Int): ULong {
        ensure(width <= input.size - offset, InviteFailure.Invalid)
        var value = 0UL
        repeat(width) { value = (value shl 8) or byte().toULong() }
        return value
    }
}

/** Canonical encoder used to require a byte-for-byte re-encoding match. */
internal class CborEncoder {
    private val output = ByteArrayOutputStream(256)

    fun array(length: Int) = head(MAJOR_ARRAY, length.toULong())

    fun uint(value: ULong) = head(MAJOR_UINT, value)

    fun optionalUint(value: ULong?) {
        if (value == null) output.write(NULL) else uint(value)
    }

    fun bytes(value: ByteArray) = string(MAJOR_BYTES, value)

    fun text(value: String) = string(MAJOR_TEXT, value.toByteArray(StandardCharsets.UTF_8))

    fun toByteArray(): ByteArray = output.toByteArray()

    private fun string(major: Int, value: ByteArray) {
        head(major, value.size.toULong())
        output.write(value)
    }

    private fun head(major: Int, value: ULong) {
        val prefix = major shl 5
        when {
            value <= 23UL -> output.write(prefix or value.toInt())
            value <= 0xffUL -> { output.write(prefix or 24); writeBigEndian(value, 1) }
            value <= 0xffffUL -> { output.write(prefix or 25); writeBigEndian(value, 2) }
            value <= 0xffff_ffffUL -> { output.write(prefix or 26); writeBigEndian(value, 4) }
            else -> { output.write(prefix or 27); writeBigEndian(value, 8) }
        }
    }

    private fun writeBigEndian(value: ULong, width: Int) {
        for (shift in (width - 1) downTo 0) output.write(((value shr (shift * 8)) and 0xffUL).toInt())
    }
}
