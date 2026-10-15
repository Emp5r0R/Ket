package com.ket.android.invite

import java.util.Base64

const val KET_APPLICATION_ID = "org.ket.access"
const val INVITE_TEXT_PREFIX = "ket:invite:"
const val CONTROL_API_VERSION = 1
private const val INVITE_FORMAT_VERSION = 1UL
private const val CAPABILITY_ACCESS_GRANT = 1UL
private const val ACCESS_CODE_BYTES = 32
private const val PAYLOAD_FIELDS = 10
private const val SIGNED_FIELDS = 2
private const val MAX_APPLICATION_ID_BYTES = 64
private const val SIGNATURE_BYTES = 64
private const val NODE_KEY_BYTES = 32
internal const val MAX_BINARY_INVITE_BYTES = 1_024
internal const val MAX_ENCODED_INVITE_TEXT_BYTES = 2_048
private const val SIGNATURE_DOMAIN = "KET-INVITE-CAPSULE-V1"

private val accessCodePattern = Regex("^[A-Za-z0-9]{32}$")
private val base64UrlAlphabet = Regex("^[A-Za-z0-9_-]+$")

/**
 * A verified Ket invite. Mirrors `ket-invite` capsule.rs and text.rs: the same checks run in the
 * same order, so both implementations report the same failure for every shared vector.
 */
class InviteCapsule private constructor(
    val nodePublicKey: NodePublicKey,
    val locators: List<ControlLocator>,
    /** The bearer access code. Never log it. */
    val accessCode: String,
    val controlApiMin: Int,
    val controlApiMax: Int,
    private val rawExpiry: ULong?,
) {
    /** Access expiry in Unix seconds, saturated to [Long.MAX_VALUE]. */
    val accessExpiresAtEpochSeconds: Long?
        get() = rawExpiry?.let { if (it > Long.MAX_VALUE.toULong()) Long.MAX_VALUE else it.toLong() }

    fun supportsControlApi(version: Int): Boolean = version in controlApiMin..controlApiMax

    override fun toString(): String =
        "InviteCapsule(node=${nodePublicKey.fingerprint}, locators=$locators, accessCode=[REDACTED], " +
            "controlApi=$controlApiMin..$controlApiMax, accessExpiresAt=$accessExpiresAtEpochSeconds)"

    companion object {
        /** Decodes and fully verifies invite text; the `ket:invite:` prefix is optional. */
        fun decodeText(input: String, nowEpochSeconds: Long?): InviteCapsule {
            ensure(input.isNotEmpty() && input.toByteArray(Charsets.UTF_8).size <= MAX_ENCODED_INVITE_TEXT_BYTES, InviteFailure.TooLarge)
            val encoded = input.removePrefix(INVITE_TEXT_PREFIX)
            ensure(base64UrlAlphabet.matches(encoded), InviteFailure.Invalid)
            val binary = try {
                Base64.getUrlDecoder().decode(encoded)
            } catch (_: IllegalArgumentException) {
                fail(InviteFailure.Invalid)
            }
            ensure(binary.size <= MAX_BINARY_INVITE_BYTES, InviteFailure.TooLarge)
            ensure(Base64Url.encode(binary) == encoded, InviteFailure.Invalid)
            return decodeBinary(binary, nowEpochSeconds)
        }

        /** Decodes and fully verifies a canonical binary invite. */
        fun decodeBinary(binary: ByteArray, nowEpochSeconds: Long?): InviteCapsule {
            ensure(binary.isNotEmpty() && binary.size <= MAX_BINARY_INVITE_BYTES, InviteFailure.TooLarge)
            val decoder = CborDecoder(binary)
            decoder.array(SIGNED_FIELDS)
            val payload = decoder.bytes(MAX_BINARY_INVITE_BYTES)
            val signature = decoder.bytes(SIGNATURE_BYTES)
            ensure(signature.size == SIGNATURE_BYTES, InviteFailure.Invalid)
            decoder.finish()
            val capsule = decodePayload(payload)
            ensure(capsule.nodePublicKey.verify(signatureInput(payload), signature), InviteFailure.InvalidSignature)
            capsule.validate(nowEpochSeconds)
            ensure(capsule.canonicalBinary(signature).contentEquals(binary), InviteFailure.Invalid)
            return capsule
        }

        private fun decodePayload(payload: ByteArray): InviteCapsule {
            val decoder = CborDecoder(payload)
            decoder.array(PAYLOAD_FIELDS)
            ensure(decoder.uint() == INVITE_FORMAT_VERSION, InviteFailure.UnsupportedVersion)
            ensure(decoder.text(MAX_APPLICATION_ID_BYTES) == KET_APPLICATION_ID, InviteFailure.WrongApplication)
            ensure(decoder.uint() == CAPABILITY_ACCESS_GRANT, InviteFailure.UnsupportedCapability)
            val nodeKeyBytes = decoder.bytes(NODE_KEY_BYTES)
            ensure(nodeKeyBytes.size == NODE_KEY_BYTES, InviteFailure.Invalid)
            val nodePublicKey = NodePublicKey.fromBytes(nodeKeyBytes)
            val locatorCount = decoder.arrayLength()
            ensure(locatorCount in 1UL..MAX_LOCATORS.toULong(), InviteFailure.Invalid)
            val locators = List(locatorCount.toInt()) { ControlLocator.decode(decoder) }
            val accessCode = decoder.text(ACCESS_CODE_BYTES)
            val controlApiMin = version(decoder)
            val controlApiMax = version(decoder)
            ensure(decoder.uint() == 0UL, InviteFailure.Invalid)
            val expiry = decoder.optionalUint()
            decoder.finish()
            val capsule = InviteCapsule(
                nodePublicKey,
                locators,
                accessCode,
                controlApiMin,
                controlApiMax,
                expiry,
            )
            ensure(capsule.canonicalPayload().contentEquals(payload), InviteFailure.Invalid)
            return capsule
        }

        private fun version(decoder: CborDecoder): Int {
            val value = decoder.uint()
            ensure(value <= 0xffffUL, InviteFailure.Invalid)
            return value.toInt()
        }

        /** `domain || u16be(len(application_id)) || application_id || payload`. */
        private fun signatureInput(payload: ByteArray): ByteArray {
            val domain = SIGNATURE_DOMAIN.toByteArray(Charsets.US_ASCII)
            val application = KET_APPLICATION_ID.toByteArray(Charsets.US_ASCII)
            return domain + byteArrayOf((application.size ushr 8).toByte(), application.size.toByte()) + application + payload
        }
    }

    private fun validate(nowEpochSeconds: Long?) {
        ensure(
            accessCodePattern.matches(accessCode) && controlApiMin != 0 && controlApiMax >= controlApiMin,
            InviteFailure.Invalid,
        )
        ControlLocator.validateSet(locators)
        val expiry = rawExpiry
        ensure(
            nowEpochSeconds == null || expiry == null || expiry > nowEpochSeconds.coerceAtLeast(0).toULong(),
            InviteFailure.Expired,
        )
    }

    private fun canonicalPayload(): ByteArray {
        validate(null)
        return CborEncoder().apply {
            array(PAYLOAD_FIELDS)
            uint(INVITE_FORMAT_VERSION)
            text(KET_APPLICATION_ID)
            uint(CAPABILITY_ACCESS_GRANT)
            bytes(nodePublicKey.encoded)
            array(locators.size)
            locators.forEach { it.encode(this) }
            text(accessCode)
            uint(controlApiMin.toULong())
            uint(controlApiMax.toULong())
            uint(0UL)
            optionalUint(rawExpiry)
        }.toByteArray()
    }

    private fun canonicalBinary(signature: ByteArray): ByteArray = CborEncoder().apply {
        array(SIGNED_FIELDS)
        bytes(canonicalPayload())
        bytes(signature)
    }.toByteArray()
}
