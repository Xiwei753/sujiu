plugins {
    id("com.android.application") version "9.4.1" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.10" apply false
}

// The Rust repository and the generated Kotlin directory are declared in the
// module script, because a Kotlin DSL script only sees its own declarations:
// sharing them from here would mean `subprojects` plumbing for two values.
