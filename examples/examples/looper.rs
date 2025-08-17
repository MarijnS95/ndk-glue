//! Demonstrates how to manage application lifetime using Android's `Looper`

use log::info;

// Don't use ndk-glue/ndk-macro startup for now.
#[no_mangle]
unsafe extern "C" fn ANativeActivity_onCreate(app: *mut ndk_sys::ANativeActivity) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Trace),
    );
    ndk_glue::init(app, std::ptr::null_mut(), 0, our_main);
}

fn our_main() {
    info!("HELLO!");
}
