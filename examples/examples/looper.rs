//! Demonstrates how to manage application lifetime using Android's `Looper`

use android_intent::{Action, Intent};
use log::{error, info};

#[no_mangle]
unsafe extern "C" fn ANativeActivity_onCreate(app: *mut ndk_sys::ANativeActivity) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    error!("LAUNCHED {app:?}");
    ndk_glue::init(app, std::ptr::null_mut(), 0, our_main);
}

fn our_main() {
    info!("HELLO!");

    // Bad hack but nice for quick iteration
    android_intent::with_current_env(|env| {
        let x = Intent::new(env, Action::Main);
        x.set_class_name("rust.example.looper", "android.app.NativeActivity")
            .start_activity()
            .unwrap()
    })
}
