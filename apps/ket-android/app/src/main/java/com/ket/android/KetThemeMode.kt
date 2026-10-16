package com.ket.android

internal enum class KetThemeMode(val storedName: String, val displayName: String) {
    System("system", "System"),
    Light("light", "Light"),
    Dark("dark", "Dark");

    companion object {
        fun fromStored(value: String?): KetThemeMode = entries.firstOrNull { it.storedName == value } ?: System
    }
}
