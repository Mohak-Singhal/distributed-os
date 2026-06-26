//! Android Agent JNI Bridge.
//!
//! Exposes a C-ABI compatible JNI function to start the agent from Kotlin/Java.

#![cfg(target_os = "android")]
#![allow(non_snake_case)]

use jni::objects::JClass;
use jni::JNIEnv;
use tracing::info;

/// Entry point called by the Android Kotlin app:
/// `external fun startAgent()`
///
/// Note: Ensure the Java class matches this package path!
#[no_mangle]
pub extern "system" fn Java_com_dos_agent_Core_startAgent(
    mut _env: JNIEnv,
    _class: JClass,
) {
    // Initialise logging (Android Logcat)
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    info!("DOS Android Agent native library loaded! Starting runtime...");

    // We spawn a new OS thread to host the Tokio runtime so we don't block
    // the calling JNI thread (which might be the main UI thread).
    std::thread::spawn(|| {
        if let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("dos-android-worker")
            .build()
        {
            rt.block_on(async {
                info!("Tokio runtime started on Android.");
                // Here we would initialise the Agent struct and call run().
                // For Phase 8/10, this is sufficient to prove compilation and JNI linking.
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    info!("Android Agent heartbeat internal ping...");
                }
            });
        }
    });
}
