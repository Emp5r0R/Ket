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

// The app version follows the workspace version in the repository's Cargo.toml;
// versionCode is MAJOR*10000 + MINOR*100 + PATCH so every release sorts higher.
val workspaceVersion = rootProject.file("../../Cargo.toml").readLines()
    .firstNotNullOfOrNull { Regex("""^version = "(\d+)\.(\d{1,2})\.(\d{1,2})"$""").find(it) }
    ?: throw GradleException("Cargo.toml has no MAJOR.MINOR.PATCH workspace version")
val workspaceVersionName = workspaceVersion.groupValues.drop(1).joinToString(".")
val workspaceVersionCode = workspaceVersion.groupValues.drop(1).map(String::toInt)
    .let { (major, minor, patch) -> major * 10000 + minor * 100 + patch }
val configuredVersionCode = System.getenv("KET_ANDROID_VERSION_CODE")?.let { value ->
    value.toIntOrNull()?.takeIf { it > 0 }
        ?: throw GradleException("KET_ANDROID_VERSION_CODE must be a positive integer")
} ?: workspaceVersionCode
val configuredVersionName = System.getenv("KET_ANDROID_VERSION_NAME")
    ?.takeIf { it.isNotBlank() }
    ?: workspaceVersionName

android { namespace = "com.ket.android"; compileSdk = 36
    ndkVersion = "27.3.13750724"
    defaultConfig {
        applicationId = "com.ket.android"; minSdk = 26; targetSdk = 36
        versionCode = configuredVersionCode
        versionName = configuredVersionName
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
    // One APK per ABI plus a universal APK. Releases ship arm64-v8a (current
    // phones) and universal (everything else); the validator checks both.
    splits {
        abi {
            isEnable = true
            reset()
            include("armeabi-v7a", "arm64-v8a", "x86", "x86_64")
            isUniversalApk = true
        }
    }
    buildTypes {
        getByName("release") {
            signingConfig = signingConfigs.findByName("release")
        }
    }
    externalNativeBuild { ndkBuild { path = file("src/main/jni/Android.mk") } }
    sourceSets {
        named("main") {
            jniLibs.srcDir(layout.buildDirectory.dir("generated/ket-engines/jniLibs"))
            jniLibs.srcDir(layout.buildDirectory.dir("generated/ket-invite/jniLibs"))
        }
        named("test") {
            resources.srcDir("src/main/res/raw")
            // Invite conformance vectors are owned by the Rust reference implementation.
            resources.srcDir(rootProject.layout.projectDirectory.dir("../../crates/ket-invite/tests/vectors"))
            // Diagnostic redaction vectors are shared with the desktop app.
            resources.srcDir(rootProject.layout.projectDirectory.dir("../vectors"))
        }
    }
    packaging { jniLibs { useLegacyPackaging = true; keepDebugSymbols += setOf("**/libhysteria.so", "**/libsslocal.so", "**/libxray.so", "**/libwstunnel.so", "**/libopenvpn.so", "**/libovpnexec.so") } }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    lint {
        abortOnError = true
        warningsAsErrors = true
        checkReleaseBuilds = true
        disable += "GradleDependency"
    }
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
        rootProject.layout.projectDirectory.dir("../..").file("packaging/patches/hev-socks5-tunnel-2.14.0-mapdns-tcp.patch"),
    )
    outputs.dir(layout.buildDirectory.dir("generated/ket-engines"))
}
// Invites are read by the shared Rust implementation (crates/ket-invite-android), built here
// for every ABI and, for the JVM unit tests, for the build machine.
val repositoryRoot = rootProject.layout.projectDirectory.dir("../..")
val inviteLibrarySources = listOf(
    repositoryRoot.file("Cargo.lock"),
    repositoryRoot.file("Cargo.toml"),
    repositoryRoot.file("packaging/build-android-invite.sh"),
    repositoryRoot.dir("crates/invite-capsule"),
    repositoryRoot.dir("crates/ket-invite"),
    repositoryRoot.dir("crates/ket-invite-android"),
)
val buildInviteLibraries by tasks.registering(Exec::class) {
    val output = layout.buildDirectory.dir("generated/ket-invite")
    inputs.files(inviteLibrarySources)
    outputs.dir(output)
    doFirst {
        commandLine(
            repositoryRoot.file("packaging/build-android-invite.sh").asFile.absolutePath,
            "android",
            android.ndkDirectory.absolutePath,
            output.get().asFile.absolutePath,
        )
    }
}
val buildHostInviteLibrary by tasks.registering(Exec::class) {
    val output = layout.buildDirectory.dir("generated/ket-invite-host")
    inputs.files(inviteLibrarySources)
    outputs.dir(output)
    commandLine(
        repositoryRoot.file("packaging/build-android-invite.sh").asFile.absolutePath,
        "host",
        output.get().asFile.absolutePath,
    )
}
tasks.withType<Test>().configureEach {
    dependsOn(buildHostInviteLibrary)
    systemProperty("java.library.path", layout.buildDirectory.dir("generated/ket-invite-host").get().asFile.absolutePath)
}
tasks.configureEach {
    if (
        name.startsWith("configureNdkBuild") ||
        name.startsWith("merge") && (name.endsWith("NativeLibs") || name.endsWith("JniLibFolders"))
    ) {
        dependsOn(prepareAndroidEngines)
    }
    if (name.startsWith("merge") && (name.endsWith("NativeLibs") || name.endsWith("JniLibFolders"))) {
        dependsOn(buildInviteLibraries)
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
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.0")
    implementation("androidx.core:core-ktx:1.13.1")
    // Invite QR scanning; same libraries and versions as Abyssal.
    implementation("com.google.zxing:core:3.5.4")
    implementation("androidx.camera:camera-camera2:1.4.2")
    implementation("androidx.camera:camera-lifecycle:1.4.2")
    implementation("androidx.camera:camera-view:1.4.2")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
}
