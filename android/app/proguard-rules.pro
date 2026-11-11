# UniFFI bindings call the native library through JNA by method name: keep them as written.
-keep class org.krunk.ffi.** { *; }
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-dontwarn java.awt.**
# GeckoView ships its own consumer rules; nothing to add.
