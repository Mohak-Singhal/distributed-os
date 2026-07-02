# Keep JSON serialization
-keepclassmembers class * {
    @org.json.JSONObject <fields>;
}

# Keep NIO transfer classes
-keep class com.dos.agent.NioTransfer$TransferResult { *; }
-keep class com.dos.agent.TransferState { *; }
-keep class com.dos.agent.TransferRecord { *; }

# Keep SSL/TLS
-keep class javax.net.ssl.** { *; }
-keep class javax.security.** { *; }
-dontwarn javax.net.ssl.**

# Keep coroutines
-keepnames class kotlinx.coroutines.internal.MainDispatcherFactory {}
-keepnames class kotlinx.coroutines.CoroutineExceptionHandler {}

# Keep Android NsdManager
-keep class android.net.nsd.** { *; }

# Keep NIO channels
-keep class java.nio.channels.** { *; }
