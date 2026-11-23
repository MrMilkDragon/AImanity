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
