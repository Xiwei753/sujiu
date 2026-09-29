# Android frontend

Native Kotlin shell for Sujiu.

The Android layer owns Android UI, lifecycle, IME behavior, secure credential storage and rendering. Shared character/world-book/prompt logic stays in Rust.

Current bootstrap intentionally uses platform views with no UI framework dependency. Rust artifacts and JNI/binding glue will be wired after the FFI contract stabilizes.

Build baseline:

- Android Gradle Plugin 9.4.x with built-in Kotlin
- Gradle 9.6+
- JDK 17
- compileSdk/targetSdk 36
- minSdk 26
