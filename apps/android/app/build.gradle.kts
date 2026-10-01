plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

/**
 * The Rust repository this app binds to.
 *
 * The generated Kotlin is produced from `crates/sujiu-uniffi`, so the crate is
 * a build input and not a vendored copy. Nothing here checks a binding into the
 * repository: a committed copy is a second place for the contract to live, and
 * it is exactly the copy that goes stale.
 */
val rustRepoRoot = generateSequence(rootProject.projectDir) { it.parentFile }
    .firstOrNull { File(it, "Cargo.toml").isFile }
    ?: error(
        "Could not find the Sujiu Rust workspace above ${rootProject.projectDir}. " +
            "The Rust sources are a build input for this module, so the build " +
            "stops here rather than generating bindings against nothing."
    )

/**
 * Where the generated Kotlin lands, inside the module build directory.
 *
 * A plain path rather than a Provider, because the Android SourceSet API
 * rejects Providers: it cannot tell Android Studio whether the directory holds
 * generated (read-only) or static (read-write) files. The task dependency is
 * carried explicitly by `preBuild` below instead of being inferred.
 */
val generatedUniffiDir = File(layout.buildDirectory.get().asFile, "generated/uniffi")

android {
    namespace = "io.sujiu.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.sujiu.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            // The UniFFI output is compiled as ordinary Kotlin. It is generated
            // rather than committed, so this directory is the only copy.
            java.directories.add(generatedUniffiDir.absolutePath)
            kotlin.directories.add(generatedUniffiDir.absolutePath)
        }
    }
}

/**
 * Generate the Kotlin bindings from the Rust app-facing API.
 *
 * UniFFI reads the compiled library rather than a separate schema file, so this
 * builds the crate with its `bindgen` feature first and then runs the generator
 * it just built. A no-op cargo build is fine here: the generator reads the
 * library that is already there, and unlike the napi declaration hook it does
 * not need the crate to have been recompiled to have something to say.
 */
val generateUniffiBindings by tasks.registering(Exec::class) {
    group = "build"
    description = "Generate Kotlin bindings from crates/sujiu-uniffi."

    workingDir = rustRepoRoot
    commandLine(
        "cargo", "build", "--quiet", "-p", "sujiu-uniffi", "--features", "bindgen",
    )

    val library = rustRepoRoot.resolve("target/debug/libsujiu_uniffi.so")
    val output = generatedUniffiDir
    commandLine(
        "cargo", "run", "--quiet", "-p", "sujiu-uniffi", "--features", "bindgen",
        "--bin", "uniffi-bindgen", "--",
        "generate", "--library", library.absolutePath,
        "--language", "kotlin",
        "--out-dir", output.absolutePath,
        "--no-format",
    )
    // ktlint is not installed here, and the generator otherwise reports its
    // absence as a warning on every build. Generated code does not have to be
    // pretty to be correct.
}

// Every compilation of this module needs the bindings, and no compilation may
// succeed against the previous run's output.
tasks.named("preBuild").configure {
    dependsOn(generateUniffiBindings)
}

dependencies {
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.activity:activity-compose:1.12.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.10.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    // The generated bindings reach the native library through JNA, so this is
    // their compile dependency rather than an optional extra. Declared here
    // because the generated code is compiled as part of this module and
    // nothing else in the app references JNA directly.
    // `jna-platform` is deliberately absent: it is a JVM-only artifact with no
    // Android variant, and the generated bindings only need the JNI bindings in
    // `jna` itself.
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    implementation("androidx.compose.ui:ui:1.11.3")
    implementation("androidx.compose.ui:ui-tooling-preview:1.11.3")
    implementation("androidx.compose.foundation:foundation:1.11.3")
    implementation("androidx.compose.material3:material3:1.4.0")
    implementation("androidx.compose.material:material-icons-core:1.7.8")

    debugImplementation("androidx.compose.ui:ui-tooling:1.11.3")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
}
