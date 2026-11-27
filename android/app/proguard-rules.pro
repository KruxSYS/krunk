# UniFFI bindings call the native library through JNA by method name: keep them as written.
-keep class org.krunk.ffi.** { *; }
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-dontwarn java.awt.**
# GeckoView ships its own consumer rules; nothing to add.

# Debug and verbose log calls are removed from release builds, the engine's included: GeckoView
# logs every page address it handles at debug level ("handleMessage ... uri=..."), private tabs
# too, and a release build should not write the user's browsing into the system log.
-assumenosideeffects class android.util.Log {
    public static int d(...);
    public static int v(...);
}

# The engine config (EngineConfig, a YAML file given to GeckoRuntimeSettings.configFilePath) is
# parsed by GeckoView with SnakeYAML into org.mozilla.gecko.util.DebugConfig, by reflection.
# SnakeYAML looks up its own package and classes by name (TypeDescription.<clinit> calls
# getPackage().getName()): renamed or repackaged it crashes the app when the runtime is created.
-keep class org.yaml.snakeyaml.** { *; }
-keeppackagenames org.yaml.snakeyaml.**
-keep class org.mozilla.gecko.util.DebugConfig { *; }
-dontwarn org.yaml.snakeyaml.**

# Casting (0.15.0, builds with Play Services): the Cast framework creates its options class from
# the name in the manifest meta-data (src/gms/AndroidManifest.xml), by reflection. The rule is
# harmless in a -Pkrunk.noGms=true build, which has no such class.
-keep class org.krunk.browser.cast.CastOptionsProvider { <init>(); }
