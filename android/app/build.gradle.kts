import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val localProps = Properties().apply {
    val file = rootProject.file("local.properties")
    if (file.exists()) file.inputStream().use { load(it) }
}
val xaraRegistered = localProps.getProperty("xara.registered") == "true" ||
    findProperty("xara.registered") == "true"

tasks.register<Exec>("cargoNdk") {
    group = "build"
    description = "Build libxara_android.so. Higher VARA speeds are included only when xara.registered=true."
    val repo = rootProject.projectDir.parentFile
    val cargoExe = System.getenv("USERPROFILE")?.let { file("$it/.cargo/bin/cargo.exe") }
    val cargo = if (cargoExe != null && cargoExe.exists()) cargoExe.absolutePath else "cargo"
    val cmd = mutableListOf(
        cargo, "ndk",
        "-t", "arm64-v8a", "-t", "armeabi-v7a", "-t", "x86_64",
        "-o", "android/app/src/main/jniLibs",
        "build", "--release", "-p", "xara-droid",
    )
    if (xaraRegistered) cmd += listOf("--features", "registered")
    commandLine(cmd)
    workingDir = repo
    environment("RUSTFLAGS", "-C link-arg=-Wl,-z,max-page-size=16384")
}

android {
    namespace = "org.x0rro.xara"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.x0rro.xara"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.1"
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-ktx:1.9.3")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("com.github.mik3y:usb-serial-for-android:3.8.0")
}
