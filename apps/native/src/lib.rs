#![cfg(target_os = "android")]

include!("workbench.rs");

#[unsafe(no_mangle)]
fn android_main(activity: android_activity::AndroidApp) {
    if let Err(error) = deployment::initialize_android(activity.clone()) {
        eprintln!("Failed to initialize Android resources: {error}");
        return;
    }
    if let Err(error) = run_workbench(activity) {
        eprintln!("Failed to run the Android workbench: {error}");
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_WorkbenchActivity_onDestroy(
    mut env: jni::JNIEnv,
    activity: jni::objects::JObject,
) {
    iced_winit::android_destroy();
    // NativeActivity's implementation sends the final lifecycle event and waits
    // for android_main to finish. Call the superclass without redispatching here.
    if let Ok(parent) = env.find_class("android/app/NativeActivity")
        && let Ok(method) = env.get_method_id(&parent, "onDestroy", "()V")
    {
        let jni = env.get_native_interface();
        unsafe {
            ((**jni).CallNonvirtualVoidMethodA.unwrap())(
                jni,
                activity.as_raw(),
                parent.as_raw(),
                method.into_raw(),
                std::ptr::null(),
            );
        }
    }
}
