#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use log::Level;
use ndk::input_queue::InputQueue;
use ndk::looper::{FdEvent, ForeignLooper, ThreadLooper};
use ndk::native_activity::NativeActivity;
use ndk::native_window::NativeWindow;
use ndk_sys::{AInputQueue, ANativeActivity, ANativeWindow, ARect};
use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use std::ffi::{CStr, CString};
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::ops::Deref;
use std::os::raw;
use std::os::unix::prelude::*;
use std::ptr::NonNull;
use std::sync::atomic::AtomicI32;
use std::sync::{Arc, Condvar, LazyLock, Mutex, OnceLock};
use std::thread;

#[cfg(feature = "logger")]
pub use android_logger;
#[cfg(feature = "logger")]
pub use log;

pub use ndk_macro::main;

/// `ndk-glue` macros register the reading end of an event pipe with the
/// main [`ThreadLooper`] under this `ident`.
/// When returned from [`ThreadLooper::poll_*`][ThreadLooper::poll_once]
/// an event can be retrieved from [`poll_events()`].
pub const NDK_GLUE_LOOPER_EVENT_PIPE_IDENT: i32 = 0;

/// The [`InputQueue`] received from Android is registered with the main
/// [`ThreadLooper`] under this `ident`.
/// When returned from [`ThreadLooper::poll_*`][ThreadLooper::poll_once]
/// an event can be retrieved from [`input_queue()`].
pub const NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT: i32 = 1;

pub fn android_log(level: Level, tag: &CStr, msg: &CStr) {
    let prio = match level {
        Level::Error => ndk_sys::android_LogPriority::ANDROID_LOG_ERROR,
        Level::Warn => ndk_sys::android_LogPriority::ANDROID_LOG_WARN,
        Level::Info => ndk_sys::android_LogPriority::ANDROID_LOG_INFO,
        Level::Debug => ndk_sys::android_LogPriority::ANDROID_LOG_DEBUG,
        Level::Trace => ndk_sys::android_LogPriority::ANDROID_LOG_VERBOSE,
    };
    unsafe {
        ndk_sys::__android_log_write(prio.0 as raw::c_int, tag.as_ptr(), msg.as_ptr());
    }
}

pub struct ActivityState {
    pub activity: NativeActivity,
    pub root_window: Option<NativeWindow>,
    pub input_queue: Option<(InputQueue, i32)>,
    pub content_rect: Rect,
}

static NATIVE_ACTIVITIES: RwLock<Vec<ActivityState>> = RwLock::new(vec![]);
static INPUT_QUEUE_IDENT: AtomicI32 = AtomicI32::new(NDK_GLUE_LOOPER_INPUT_QUEUE_IDENT);

// static NATIVE_ACTIVITY: RwLock<Option<NativeActivity>> = RwLock::new(None);
// static NATIVE_WINDOW: RwLock<Option<NativeWindow>> = RwLock::new(None);
// static INPUT_QUEUE: RwLock<Option<InputQueue>> = RwLock::new(None);
// static CONTENT_RECT: RwLock<Rect> = RwLock::new(Rect::empty());
// We share one looper and one thread for all activities.
// TODO: ForeignLooper is Send/Sync. Does not need to be in a Mutex, except for initial state assignment?
// If so, this static should be shared by all activities
static LOOPER: Mutex<Option<ForeignLooper>> = Mutex::new(None);

// /// This function accesses a `static` variable internally and must only be used if you are sure
// /// there is exactly one version of [`ndk_glue`][crate] in your dependency tree.
// ///
// /// If you need access to the `JavaVM` through [`NativeActivity::vm()`] or Activity `Context`
// /// through [`NativeActivity::activity()`], please use the [`ndk_context`] crate and its
// /// [`ndk_context::android_context()`] getter to acquire the `JavaVM` and `Context` instead.
// pub fn native_activity() -> Option<LockReadGuard<NativeActivity>> {
//     LockReadGuard::from_wrapped_option(NATIVE_ACTIVITY.read())
// }

pub fn activities() -> RwLockReadGuard<'static, Vec<ActivityState>> {
    // XXX: Don't let the user read?
    NATIVE_ACTIVITIES.read()
}

pub fn activity_state_mut(
    activity: *mut ANativeActivity,
) -> Option<MappedRwLockWriteGuard<'static, ActivityState>> {
    let activities = NATIVE_ACTIVITIES.write();
    RwLockWriteGuard::try_map(activities, |a: &mut Vec<ActivityState>| {
        a.iter_mut().find(|a| a.activity.ptr().as_ptr() == activity)
    })
    .ok()
}

pub fn activity_state_by_input_queue_ident_mut(
    ident: i32,
) -> Option<MappedRwLockWriteGuard<'static, ActivityState>> {
    let activities = NATIVE_ACTIVITIES.write();
    RwLockWriteGuard::try_map(activities, |a: &mut Vec<ActivityState>| {
        a.iter_mut()
            .find(|a| a.input_queue.as_ref().is_some_and(|&(_, i)| i == ident))
    })
    .ok()
}

pub struct LockReadGuard<T: ?Sized + 'static>(MappedRwLockReadGuard<'static, T>);

impl<T> LockReadGuard<T> {
    /// Transpose an [`Option`] wrapped inside a [`LockReadGuard`]
    ///
    /// This is a _read_ lock for which the contents can't change; hence allowing the user to only
    /// check for [`None`] once and hold a lock containing `T` directly thereafter, without
    /// subsequent infallible [`Option::unwrap()`]s.
    fn from_wrapped_option(wrapped: RwLockReadGuard<'static, Option<T>>) -> Option<Self> {
        RwLockReadGuard::try_map(wrapped, Option::as_ref)
            .ok()
            .map(Self)
    }
}

impl<T: ?Sized> Deref for LockReadGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for LockReadGuard<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for LockReadGuard<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// /// Returns a [`NativeWindow`] held inside a lock, preventing Android from freeing it immediately
// /// in [its `NativeWindow` destructor].
// ///
// /// If the window is in use by e.g. a graphics API, make sure to hold on to this lock.
// ///
// /// After receiving [`Event::WindowDestroyed`] `ndk-glue` will block in Android's [`NativeWindow`] destructor
// /// callback until the lock is released, returning to Android and allowing it to free the window.
// ///
// /// [its `NativeWindow` destructor]: https://developer.android.com/ndk/reference/struct/a-native-activity-callbacks#onnativewindowdestroyed
// ///
// /// # Warning
// /// This function accesses a `static` variable internally and must only be used if you are sure
// /// there is exactly one version of `ndk_glue` in your dependency tree.
// pub fn native_window() -> Option<LockReadGuard<NativeWindow>> {
//     LockReadGuard::from_wrapped_option(NATIVE_WINDOW.read())
// }

// /// Returns an [`InputQueue`] held inside a lock, preventing Android from freeing it immediately
// /// in [its `InputQueue` destructor].
// ///
// /// After receiving [`Event::InputQueueDestroyed`] `ndk-glue` will block in Android's [`InputQueue`] destructor
// /// callback until the lock is released, returning to Android and allowing it to free the window.
// ///
// /// [its `InputQueue` destructor]: https://developer.android.com/ndk/reference/struct/a-native-activity-callbacks#oninputqueuedestroyed
// ///
// /// # Warning
// /// This function accesses a `static` variable internally and must only be used if you are sure
// /// there is exactly one version of `ndk_glue` in your dependency tree.
// pub fn input_queue() -> Option<LockReadGuard<InputQueue>> {
//     LockReadGuard::from_wrapped_option(INPUT_QUEUE.read())
// }

// /// This function accesses a `static` variable internally and must only be used if you are sure
// /// there is exactly one version of `ndk_glue` in your dependency tree.
// pub fn content_rect() -> Rect {
//     CONTENT_RECT.read().clone()
// }

static PIPE: LazyLock<(OwnedFd, OwnedFd)> = LazyLock::new(|| rustix::pipe::pipe().unwrap());

pub fn poll_events() -> Option<(*mut ANativeActivity, Event)> {
    unsafe {
        let mut event = [0u8];
        let res = rustix::io::read(&PIPE.0, &mut event).unwrap();
        assert_eq!(res, event.len());
        let mut activity = 0usize.to_le_bytes();
        let res = rustix::io::read(&PIPE.0, &mut activity).unwrap();
        assert_eq!(res, activity.len());
        // {
        Some((
            usize::from_le_bytes(activity) as *mut ANativeActivity,
            std::mem::transmute::<u8, Event>(event[0]),
        ))
        // } else {
        //     None
        // }
    }
}

unsafe fn wake(activity: *mut ANativeActivity, event: Event) {
    log::trace!("Wake {activity:p} {event:?}");

    let event = [event as u8];
    let res = rustix::io::write(&PIPE.1, &event).unwrap();
    assert_eq!(res, event.len());
    let activity = (activity as usize).to_le_bytes();
    let res = rustix::io::write(&PIPE.1, &activity).unwrap();
    assert_eq!(res, activity.len());
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl Rect {
    pub const fn empty() -> Self {
        Self {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Event {
    Start,
    Resume,
    SaveInstanceState,
    Pause,
    Stop,
    /// The native activity will be stopped and destroyed after this event.
    /// Due to the async nature of these events, make sure to hold on to the
    /// lock received from [`native_activity()`] _beforehand_ if you wish to use
    /// it during handling of [`Event::Destroy`]. The lock should be released to
    /// allow `onDestroy` to return.
    Destroy,
    ConfigChanged,
    LowMemory,
    WindowLostFocus,
    WindowHasFocus,
    /// A [`NativeWindow`] is now available through [`native_window()`]. See that function for more
    /// details about holding on to the returned [`LockReadGuard`].
    ///
    /// Be sure to release any resources (e.g. Vulkan/OpenGL graphics surfaces) created from
    /// it followed by releasing this lock upon receiving [`Event::WindowDestroyed`].
    WindowCreated,
    WindowResized,
    WindowRedrawNeeded,
    /// If the window is in use by e.g. a graphics API, make sure the [`LockReadGuard`] from
    /// [`native_window()`] is held on to until after freeing those resources.
    ///
    /// After receiving this [`Event`] `ndk_glue` will block inside its [`NativeWindow`] destructor
    /// until that read-lock is released before returning to Android and allowing it to free the
    /// window.
    ///
    /// From this point [`native_window()`] will return [`None`] until receiving
    /// [`Event::WindowCreated`] again.
    WindowDestroyed,
    /// An [`InputQueue`] is now available through [`input_queue()`].
    ///
    /// Be sure to release the returned lock upon receiving [`Event::InputQueueDestroyed`].
    InputQueueCreated,
    /// After receiving this [`Event`] `ndk_glue` will block inside its [`InputQueue`] destructor
    /// until the read-lock from [`input_queue()`] is released before returning to Android and
    /// allowing it to free the input queue.
    ///
    /// From this point [`input_queue()`] will return [`None`] until receiving
    /// [`Event::InputQueueCreated`] again.
    InputQueueDestroyed,
    ContentRectChanged,
}

/// # Safety
/// `activity` must either be null (resulting in a safe panic)
/// or a pointer to a valid Android `ANativeActivity`.
pub unsafe fn init(
    activity: *mut ANativeActivity,
    _saved_state: *mut u8,
    _saved_state_size: usize,
    main: fn(),
) {
    let mut activity = NonNull::new(activity).unwrap();
    let callbacks = activity.as_mut().callbacks.as_mut().unwrap();
    callbacks.onStart = Some(on_start);
    callbacks.onResume = Some(on_resume);
    callbacks.onSaveInstanceState = Some(on_save_instance_state);
    callbacks.onPause = Some(on_pause);
    callbacks.onStop = Some(on_stop);
    callbacks.onDestroy = Some(on_destroy);
    callbacks.onWindowFocusChanged = Some(on_window_focus_changed);
    callbacks.onNativeWindowCreated = Some(on_window_created);
    callbacks.onNativeWindowResized = Some(on_window_resized);
    callbacks.onNativeWindowRedrawNeeded = Some(on_window_redraw_needed);
    callbacks.onNativeWindowDestroyed = Some(on_window_destroyed);
    callbacks.onInputQueueCreated = Some(on_input_queue_created);
    callbacks.onInputQueueDestroyed = Some(on_input_queue_destroyed);
    callbacks.onContentRectChanged = Some(on_content_rect_changed);
    callbacks.onConfigurationChanged = Some(on_configuration_changed);
    callbacks.onLowMemory = Some(on_low_memory);

    let activity = NativeActivity::from_ptr(activity);
    // ndk_context::initialize_android_context(activity.vm().cast(), activity.activity().cast());
    NATIVE_ACTIVITIES.write().push(ActivityState {
        activity,
        root_window: None,
        input_queue: None,
        content_rect: Rect::empty(),
    });

    static LOG_FORWARDER: LazyLock<thread::JoinHandle<std::io::Result<()>>> = LazyLock::new(|| {
        let file = {
            let (read, write) = rustix::pipe::pipe().unwrap();
            rustix::stdio::dup2_stdout(&write).unwrap();
            rustix::stdio::dup2_stderr(&write).unwrap();

            File::from(read)
        };

        thread::spawn(move || -> std::io::Result<()> {
            let mut reader = BufReader::new(file);
            let mut buffer = String::new();
            loop {
                buffer.clear();
                let len = reader.read_line(&mut buffer)?;
                if len == 0 {
                    break Ok(());
                } else if let Ok(msg) = CString::new(buffer.clone()) {
                    android_log(Level::Info, c"RustStdoutStderr", &msg);
                    // log::info!(target: "RustStdoutStderr", "{buffer}");
                }
            }
        })
    });

    static MAIN_THREAD: OnceLock<thread::JoinHandle<()>> = OnceLock::new();

    MAIN_THREAD.get_or_init(move || {
        let looper_ready = Arc::new(Condvar::new());
        let signal_looper_ready = looper_ready.clone();

        let jh = thread::spawn(move || {
            let looper = ThreadLooper::prepare();
            // TODO: Why didn't we Deref ThreadLooper into ForeignLooper? The latter is more restrictive.
            let foreign = looper.into_foreign();
            foreign
                .add_fd(
                    // TODO: Take impl AsFd.
                    PIPE.0.as_fd(),
                    // &PIPE.0,
                    NDK_GLUE_LOOPER_EVENT_PIPE_IDENT,
                    FdEvent::INPUT,
                    std::ptr::null_mut(),
                )
                .unwrap();

            {
                let mut locked_looper = LOOPER.lock().unwrap();
                let previous = locked_looper.replace(foreign);
                assert!(previous.is_none(), "LazyLock is running twice?");
                signal_looper_ready.notify_one();
            }

            // TODO: We won't call the users' main function more often. They just need to listen to the looper
            // TODO: Give them the looper at least
            main()
        });

        // Don't return from this function (`ANativeActivity_onCreate`) until the thread
        // has created its `ThreadLooper` and assigned it to the static `LOOPER`
        // variable. It will be used from `on_input_queue_created` as soon as this
        // function returns.
        let locked_looper = LOOPER.lock().unwrap();
        let _mutex_guard = looper_ready
            .wait_while(locked_looper, |looper| looper.is_none())
            .unwrap();

        jh
    });
}

unsafe extern "C" fn on_start(activity: *mut ANativeActivity) {
    wake(activity, Event::Start);
}

unsafe extern "C" fn on_resume(activity: *mut ANativeActivity) {
    wake(activity, Event::Resume);
}

unsafe extern "C" fn on_save_instance_state(
    activity: *mut ANativeActivity,
    _out_size: *mut usize,
) -> *mut raw::c_void {
    // TODO
    wake(activity, Event::SaveInstanceState);
    std::ptr::null_mut()
}

unsafe extern "C" fn on_pause(activity: *mut ANativeActivity) {
    wake(activity, Event::Pause);
}

unsafe extern "C" fn on_stop(activity: *mut ANativeActivity) {
    wake(activity, Event::Stop);
}

unsafe extern "C" fn on_destroy(activity: *mut ANativeActivity) {
    log::error!("Destroyed {activity:?}");
    wake(activity, Event::Destroy);
    // ndk_context::release_android_context();
    let mut native_activity_guard = NATIVE_ACTIVITIES.write();
    let idx = native_activity_guard
        .iter()
        .position(|a| a.activity.ptr().as_ptr() == activity)
        .unwrap();
    let _deleted = native_activity_guard.swap_remove(idx);
}

unsafe extern "C" fn on_configuration_changed(activity: *mut ANativeActivity) {
    wake(activity, Event::ConfigChanged);
}

unsafe extern "C" fn on_low_memory(activity: *mut ANativeActivity) {
    wake(activity, Event::LowMemory);
}

unsafe extern "C" fn on_window_focus_changed(
    activity: *mut ANativeActivity,
    has_focus: raw::c_int,
) {
    let event = if has_focus == 0 {
        Event::WindowLostFocus
    } else {
        Event::WindowHasFocus
    };
    wake(activity, event);
}

unsafe extern "C" fn on_window_created(activity: *mut ANativeActivity, window: *mut ANativeWindow) {
    let mut state = activity_state_mut(activity).unwrap();
    let previous = state
        .root_window
        .replace(NativeWindow::clone_from_ptr(NonNull::new(window).unwrap()));
    assert!(previous.is_none());
    wake(activity, Event::WindowCreated);
}

unsafe extern "C" fn on_window_resized(
    activity: *mut ANativeActivity,
    _window: *mut ANativeWindow,
) {
    wake(activity, Event::WindowResized);
}

unsafe extern "C" fn on_window_redraw_needed(
    activity: *mut ANativeActivity,
    _window: *mut ANativeWindow,
) {
    wake(activity, Event::WindowRedrawNeeded);
}

unsafe extern "C" fn on_window_destroyed(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    wake(activity, Event::WindowDestroyed);
    let mut state = activity_state_mut(activity).unwrap();

    assert_eq!(state.root_window.take().unwrap().ptr().as_ptr(), window);
}

unsafe extern "C" fn on_input_queue_created(
    activity: *mut ANativeActivity,
    queue: *mut AInputQueue,
) {
    let input_queue = InputQueue::from_ptr(NonNull::new(queue).unwrap());
    let locked_looper = LOOPER.lock().unwrap();
    // The looper should always be `Some` after `fn init()` returns, unless
    // future code cleans it up and sets it back to `None` again.
    let looper = locked_looper.as_ref().expect("Looper does not exist");

    // TODO: This index will be invalidated when the activity closes...
    let mut a = NATIVE_ACTIVITIES.write();
    let ident = INPUT_QUEUE_IDENT.fetch_add(1, std::sync::atomic::Ordering::Release);
    input_queue.attach_looper(looper, ident);

    let mut state = activity_state_mut(activity).unwrap();
    // let state = &mut a[idx];
    let previous = state.input_queue.replace((input_queue, ident));
    assert!(previous.is_none());
    wake(activity, Event::InputQueueCreated);
}

unsafe extern "C" fn on_input_queue_destroyed(
    activity: *mut ANativeActivity,
    queue: *mut AInputQueue,
) {
    wake(activity, Event::InputQueueDestroyed);
    let mut state = activity_state_mut(activity).unwrap();
    let (input_queue, _ident) = state.input_queue.take().unwrap();
    assert_eq!(input_queue.ptr().as_ptr(), queue);
    input_queue.detach_looper();
}

unsafe extern "C" fn on_content_rect_changed(activity: *mut ANativeActivity, rect: *const ARect) {
    let rect = Rect {
        left: (*rect).left as _,
        top: (*rect).top as _,
        right: (*rect).right as _,
        bottom: (*rect).bottom as _,
    };
    let mut state = activity_state_mut(activity).unwrap();
    state.content_rect = rect;
    wake(activity, Event::ContentRectChanged);
}
