//! Demonstrates how to manage application lifetime using Android's `Looper`

use std::ptr::NonNull;

use android_intent::{Action, Intent};
use log::{error, info};
use ndk::{looper::ThreadLooper, native_activity::NativeActivity};
use ndk_glue::{Event, NDK_GLUE_LOOPER_EVENT_PIPE_IDENT, NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT};

#[no_mangle]
unsafe extern "C" fn ANativeActivity_onCreate(app: *mut ndk_sys::ANativeActivity) {
    std::env::set_var("RUST_BACKTRACE", "1");
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    error!("LAUNCHED {app:?}");
    ndk_glue::init(app, std::ptr::null_mut(), 0, our_main);
}

fn our_main() {
    info!("HELLO!");

    // let app = ndk_glue::native_activity().unwrap();
    // let looper = ndk_glue::NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT
    let looper = ThreadLooper::for_thread().unwrap();

    while let Ok(event) = looper.poll_all() {
        info!("{event:?}");
        match event {
            ndk::looper::Poll::Event {
                ident: NDK_GLUE_LOOPER_EVENT_PIPE_IDENT,
                fd,
                events,
                data,
            } => {
                let x = ndk_glue::poll_events();
                info!("Read event {x:?}");
                match x {
                    Some((act, Event::Start)) => {
                        info!("Activity {act:?} started");
                    }
                    Some((act, Event::WindowHasFocus)) => {
                        // let act = unsafe { NativeActivity::from_ptr(NonNull::new(act).unwrap()) };
                    }
                    _ => {}
                }
            }
            ndk::looper::Poll::Event {
                ident,
                fd,
                events,
                data,
            } => {
                // let idx = ident - NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT;
                // info!("IQ at index {idx:?}");
                let state = ndk_glue::activity_state_by_input_queue_ident_mut(ident).unwrap();
                let (iq, ident) = state.input_queue.as_ref().unwrap();
                let e = dbg!(iq.event()).unwrap().unwrap();
                dbg!(&e);
                let launch = match &e {
                    ndk::event::InputEvent::KeyEvent(k)
                        if k.key_code() == ndk::event::Keycode::VolumeUp
                            && k.action() == ndk::event::KeyAction::Down =>
                    {
                        true
                    }
                    _ => false,
                };
                iq.finish_event(e, launch);

                if launch {
                    info!("Launch new activity");
                    let act = &state.activity;
                    // Bad hack but nice for quick iteration
                    // android_intent::with_current_env(|env| {
                    let vm = unsafe { jni::JavaVM::from_raw(act.vm()) }.unwrap();
                    let mut env = vm.attach_current_thread().unwrap();
                    let activity = unsafe { jni::objects::JObject::from_raw(act.activity()) };
                    const FLAG_ACTIVITY_NEW_TASK: i32 = 0x10000000;
                    const FLAG_ACTIVITY_LAUNCH_ADJACENT: i32 = 0x00001000;
                    const FLAG_ACTIVITY_MULTIPLE_TASK: i32 = 0x08000000;
                    Intent::new(&mut env, Action::Main)
                        .set_class_name("rust.example.looper", "android.app.NativeActivity")
                        .add_flags(
                            FLAG_ACTIVITY_NEW_TASK
                                | FLAG_ACTIVITY_LAUNCH_ADJACENT
                                | FLAG_ACTIVITY_MULTIPLE_TASK,
                        )
                        .start_activity(activity)
                        .unwrap()
                    // })
                }
            }
            _ => {}
        }
    }
}
