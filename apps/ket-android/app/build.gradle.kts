import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android { namespace = "com.ket.android"; compileSdk = 34
    ndkVersion = "27.3.13750724"
    defaultConfig {
        applicationId = "com.ket.android"; minSdk = 26; targetSdk = 34; versionCode = 1; versionName = "0.1.0"
        ndk { abiFilters += setOf("armeabi-v7a", "arm64-v8a", "x86", "x86_64") }
        externalNativeBuild {
            ndkBuild { arguments += "NDK_APPLICATION_MK:=${file("src/main/jni/Application.mk").absolutePath}" }
        }
    }
    externalNativeBuild { ndkBuild { path = file("src/main/jni/Android.mk") } }
    sourceSets { named("main") { jniLibs.srcDir(layout.buildDirectory.dir("generated/ket-engines/jniLibs")) } }
    packaging { jniLibs { useLegacyPackaging = true; keepDebugSymbols += "**/libhysteria.so" } }
    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.11" }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
}
val prepareAndroidEngines by tasks.registering(Exec::class) {
    val script = rootProject.layout.projectDirectory.dir("../..").file("packaging/prepare-android-engines.sh")
    commandLine(script.asFile.absolutePath, project.layout.projectDirectory.asFile.absolutePath)
    inputs.files(script, rootProject.layout.projectDirectory.dir("../..").file("packaging/fetch-hysteria.sh"))
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
