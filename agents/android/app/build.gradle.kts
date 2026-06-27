plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.dos.agent"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.dos.agent"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    
    sourceSets {
        getByName("main") {
            jniLibs.srcDir("src/main/jniLibs")
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("com.google.android.material:material:1.11.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.7.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")
}

tasks.register<Exec>("cargoBuild") {
    workingDir = file("../")
    
    val isRelease = gradle.startParameter.taskNames.any { it.contains("Release") }
    
    val cargoCommand = mutableListOf("cargo", "ndk", "-o", "./app/src/main/jniLibs", "-t", "arm64-v8a", "-t", "armeabi-v7a", "-t", "x86", "-t", "x86_64", "build", "-p", "dos-android")
    if (isRelease) {
        cargoCommand.add("--release")
    }

    commandLine = cargoCommand
}

tasks.whenTaskAdded {
    if (name.startsWith("merge") && name.endsWith("JniLibFolders")) {
        dependsOn("cargoBuild")
    }
}
