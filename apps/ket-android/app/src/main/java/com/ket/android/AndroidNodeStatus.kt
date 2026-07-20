package com.ket.android

import org.json.JSONObject

enum class AndroidNodeHealth(val wireName: String, val displayName: String) {
    Healthy("healthy", "Healthy"),
    Degraded("degraded", "Degraded"),
    Saturated("saturated", "Saturated");

    companion object {
        fun parse(value: String): AndroidNodeHealth = entries.firstOrNull { it.wireName == value }
            ?: throw IllegalArgumentException("Node health is invalid")
    }
}

data class AndroidNodeLocation(
    val countryCode: String,
    val countryName: String,
    val city: String?,
    val latitude: Double,
    val longitude: Double,
) {
    val displayName: String
        get() = listOfNotNull(city, countryName).distinct().joinToString(", ")
}

data class AndroidNodeStatus(
    val id: String,
    val displayName: String,
    val publicUrl: String,
    val location: AndroidNodeLocation,
    val health: AndroidNodeHealth,
    val activeSessions: Int,
    val sessionCapacity: Int,
    val capacityPercent: Double,
    val cpuLoadPercent: Double?,
    val memoryUsedBytes: Long?,
    val memoryTotalBytes: Long?,
    val uptimeSeconds: Long?,
    val observedAtEpochSeconds: Long,
)

internal object AndroidNodeStatusParser {
    private val countryCodePattern = Regex("^[A-Z]{2}$")

    fun parse(json: JSONObject): AndroidNodeStatus {
        val location = parseLocation(json.getJSONObject("location"))
        val activeSessions = nonNegativeInt(json, "active_sessions")
        val sessionCapacity = json.getInt("session_capacity").also {
            require(it > 0) { "Node session capacity must be positive" }
        }
        require(activeSessions <= sessionCapacity) { "Node active sessions exceed capacity" }
        val capacityPercent = finiteDouble(json, "capacity_percent").also {
            require(it in 0.0..100.0) { "Node capacity percent is invalid" }
        }
        val cpuLoadPercent = optionalFiniteDouble(json, "cpu_load_percent")?.also {
            require(it in 0.0..100.0) { "Node CPU load is invalid" }
        }
        val memoryUsedBytes = optionalNonNegativeLong(json, "memory_used_bytes")
        val memoryTotalBytes = optionalNonNegativeLong(json, "memory_total_bytes")
        require((memoryUsedBytes == null) == (memoryTotalBytes == null)) {
            "Node memory telemetry is incomplete"
        }
        if (memoryUsedBytes != null && memoryTotalBytes != null) {
            require(memoryTotalBytes > 0 && memoryUsedBytes <= memoryTotalBytes) {
                "Node memory telemetry is invalid"
            }
        }
        return AndroidNodeStatus(
            id = boundedText(json.getString("node_id"), "Node ID", 128),
            displayName = boundedText(json.getString("display_name"), "Node name", 128),
            publicUrl = boundedText(json.getString("public_url"), "Node public URL", 2048),
            location = location,
            health = AndroidNodeHealth.parse(json.getString("health")),
            activeSessions = activeSessions,
            sessionCapacity = sessionCapacity,
            capacityPercent = capacityPercent,
            cpuLoadPercent = cpuLoadPercent,
            memoryUsedBytes = memoryUsedBytes,
            memoryTotalBytes = memoryTotalBytes,
            uptimeSeconds = optionalNonNegativeLong(json, "uptime_seconds"),
            observedAtEpochSeconds = json.getLong("observed_at_epoch_seconds").also {
                require(it > 0) { "Node observation time is invalid" }
            },
        )
    }

    private fun parseLocation(json: JSONObject): AndroidNodeLocation {
        val countryCode = json.getString("country_code").uppercase().also {
            require(countryCodePattern.matches(it)) { "Node country code is invalid" }
        }
        val latitude = finiteDouble(json, "latitude").also {
            require(it in -90.0..90.0) { "Node latitude is invalid" }
        }
        val longitude = finiteDouble(json, "longitude").also {
            require(it in -180.0..180.0) { "Node longitude is invalid" }
        }
        val city = if (!json.has("city") || json.isNull("city")) {
            null
        } else {
            boundedText(json.getString("city"), "Node city", 128)
        }
        return AndroidNodeLocation(
            countryCode = countryCode,
            countryName = boundedText(json.getString("country_name"), "Node country", 128),
            city = city,
            latitude = latitude,
            longitude = longitude,
        )
    }

    private fun boundedText(value: String, label: String, maximumLength: Int): String =
        value.trim().also {
            require(it.isNotEmpty() && it.length <= maximumLength && it.none(Char::isISOControl)) {
                "$label is invalid"
            }
        }

    private fun finiteDouble(json: JSONObject, key: String): Double = json.getDouble(key).also {
        require(it.isFinite()) { "$key is invalid" }
    }

    private fun optionalFiniteDouble(json: JSONObject, key: String): Double? =
        if (!json.has(key) || json.isNull(key)) null else finiteDouble(json, key)

    private fun nonNegativeInt(json: JSONObject, key: String): Int = json.getInt(key).also {
        require(it >= 0) { "$key is invalid" }
    }

    private fun optionalNonNegativeLong(json: JSONObject, key: String): Long? =
        if (!json.has(key) || json.isNull(key)) {
            null
        } else {
            json.getLong(key).also { require(it >= 0) { "$key is invalid" } }
        }
}
