import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val releaseSigningEnvironment = mapOf(
    "storeFile" to System.getenv("KET_ANDROID_KEYSTORE"),
    "storePassword" to System.getenv("KET_ANDROID_KEYSTORE_PASSWORD"),
    "keyAlias" to System.getenv("KET_ANDROID_KEY_ALIAS"),
    "keyPassword" to System.getenv("KET_ANDROID_KEY_PASSWORD"),
)
val releaseSigningConfigured = releaseSigningEnvironment.values.any { !it.isNullOrBlank() }
val releaseSigningComplete = releaseSigningEnvironment.values.all { !it.isNullOrBlank() }

if (releaseSigningConfigured && !releaseSigningComplete) {
    val missing = releaseSigningEnvironment.filterValues { it.isNullOrBlank() }.keys.sorted()
    throw GradleException("Incomplete Android release signing environment; missing: ${missing.joinToString()}")
}

val configuredVersionCode = System.getenv("KET_ANDROID_VERSION_CODE")?.let { value ->
    value.toIntOrNull()?.takeIf { it > 0 }
        ?: throw GradleException("KET_ANDROID_VERSION_CODE must be a positive integer")
} ?: 1
val configuredVersionName = System.getenv("KET_ANDROID_VERSION_NAME")
    ?.takeIf { it.isNotBlank() }
    ?: "0.1.0"

android { namespace = "com.ket.android"; compileSdk = 34
    ndkVersion = "27.3.13750724"
    defaultConfig {
        applicationId = "com.ket.android"; minSdk = 26; targetSdk = 34
        versionCode = configuredVersionCode
        versionName = configuredVersionName
        ndk { abiFilters += setOf("armeabi-v7a", "arm64-v8a", "x86", "x86_64") }
        externalNativeBuild {
            ndkBuild { arguments += "NDK_APPLICATION_MK:=${file("src/main/jni/Application.mk").absolutePath}" }
        }
    }
    signingConfigs {
        if (releaseSigningComplete) {
            create("release") {
                storeFile = file(requireNotNull(releaseSigningEnvironment["storeFile"]))
                storePassword = releaseSigningEnvironment["storePassword"]
                keyAlias = releaseSigningEnvironment["keyAlias"]
                keyPassword = releaseSigningEnvironment["keyPassword"]
            }
        }
    }
    buildTypes {
        getByName("release") {
            signingConfig = signingConfigs.findByName("release")
        }
    }
    externalNativeBuild { ndkBuild { path = file("src/main/jni/Android.mk") } }
    sourceSets {
        named("main") { jniLibs.srcDir(layout.buildDirectory.dir("generated/ket-engines/jniLibs")) }
        named("test") { resources.srcDir("src/main/res/raw") }
    }
    packaging { jniLibs { useLegacyPackaging = true; keepDebugSymbols += setOf("**/libhysteria.so", "**/libsslocal.so", "**/libxray.so", "**/libwstunnel.so") } }
    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.11" }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
}
val prepareAndroidEngines by tasks.registering(Exec::class) {
    val script = rootProject.layout.projectDirectory.dir("../..").file("packaging/prepare-android-engines.sh")
    commandLine(script.asFile.absolutePath, project.layout.projectDirectory.asFile.absolutePath)
    inputs.files(
        script,
        rootProject.layout.projectDirectory.dir("../..").file("packaging/fetch-hysteria.sh"),
        rootProject.layout.projectDirectory.dir("../..").file("packaging/fetch-shadowsocks.sh"),
        rootProject.layout.projectDirectory.dir("../..").file("packaging/fetch-xray.sh"),
        rootProject.layout.projectDirectory.dir("../..").file("packaging/fetch-wstunnel.sh"),
    )
    outputs.dir(layout.buildDirectory.dir("generated/ket-engines"))
}
tasks.configureEach {
    if (
        name.startsWith("configureNdkBuild") ||
        name.startsWith("merge") && (name.endsWith("NativeLibs") || name.endsWith("JniLibFolders"))
    ) {
        dependsOn(prepareAndroidEngines)
    }
}
tasks.withType<KotlinCompile>().configureEach { kotlinOptions.jvmTarget = "17" }
gradle.taskGraph.whenReady {
    val buildsRelease = allTasks.any { task ->
        task.project == project && task.name.contains("release", ignoreCase = true)
    }
    if (buildsRelease && !releaseSigningComplete) {
        throw GradleException(
            "Android release tasks require KET_ANDROID_KEYSTORE, KET_ANDROID_KEYSTORE_PASSWORD, " +
                "KET_ANDROID_KEY_ALIAS, and KET_ANDROID_KEY_PASSWORD",
        )
    }
}
dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.05.00"))
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.0")
    implementation("androidx.core:core-ktx:1.13.1")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
}
