//! The iPhone shell's way in: a small C interface the Swift app (`apps/mac/ios`) calls from the
//! main thread. Swift owns the window, the `CAMetalLayer`, the display link and the keyboard;
//! Rune draws the app into the layer and takes touches and typed text.

use std::ffi::{CStr, c_char, c_void};
use std::sync::Arc;

use rune::embed::LayerHost;
use rune::{Ime, Key, KeyEvent, Mods, TouchPhase};

use crate::state::App;

/// One running app inside the shell's view.
pub struct OnHost {
    host: LayerHost<App>,
}

/// What the shell does after a frame.
#[repr(C)]
pub struct OnFrame {
    /// Keep the display link running: something is moving or input is pending.
    pub animating: bool,
    /// A text field has focus, so the keyboard should be up.
    pub wants_keyboard: bool,
    /// Seconds until a timer wants a frame, or a negative number for none.
    pub wake_after: f64,
}

/// Called from any thread when work finishes and the app wants a frame.
pub type OnWake = extern "C" fn(ctx: *mut c_void);

struct WakeCtx(*mut c_void);
// The shell's context is an object it keeps alive for the host's life and that is safe to
// message from any thread (it only hops to the main queue).
unsafe impl Send for WakeCtx {}
unsafe impl Sync for WakeCtx {}

/// Starts the app in `layer` (a `CAMetalLayer *`) at `width` x `height` points.
///
/// # Safety
/// Call on the main thread. `layer` must be a valid `CAMetalLayer` that outlives the host,
/// `data_dir` a NUL-terminated UTF-8 path, and `ctx` valid for as long as `wake` may be called.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_create(
    layer: *mut c_void,
    width: f32,
    height: f32,
    scale: f32,
    dark: bool,
    data_dir: *const c_char,
    wake: OnWake,
    ctx: *mut c_void,
) -> *mut OnHost {
    if !data_dir.is_null() {
        let dir = unsafe { CStr::from_ptr(data_dir) }.to_string_lossy().into_owned();
        // Before anything reads it: the engine and settings keep everything in the app's sandbox.
        unsafe { std::env::set_var("OPEN_NOTEBOOK_DATA_DIR", dir) };
    }
    let spec = crate::spec();
    let state = (spec.init)();
    let ctx = Arc::new(WakeCtx(ctx));
    let waker: rune::Waker = Arc::new(move || wake(ctx.0));
    match unsafe { LayerHost::from_core_animation_layer(layer, &spec, state, (width, height), scale, dark, waker) } {
        Ok(host) => Box::into_raw(Box::new(OnHost { host })),
        Err(e) => {
            eprintln!("open notebook: could not start: {e}");
            std::ptr::null_mut()
        }
    }
}

/// Draws one frame.
///
/// # Safety
/// `h` comes from `on_create`; main thread only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_frame(h: *mut OnHost) -> OnFrame {
    let Some(h) = (unsafe { h.as_mut() }) else { return OnFrame { animating: false, wants_keyboard: false, wake_after: -1.0 } };
    let f = h.host.frame();
    let rt = h.host.runtime();
    OnFrame { animating: f.animating, wants_keyboard: rt.ime_area.is_some(), wake_after: f.wake_after_secs.unwrap_or(-1.0) }
}

/// The view changed size, or the app went to the background (0 x 0 stops drawing).
///
/// # Safety
/// As `on_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_resize(h: *mut OnHost, width: f32, height: f32, scale: f32) {
    if let Some(h) = unsafe { h.as_mut() } {
        h.host.resize((width, height), scale);
    }
}

/// A finger: `phase` 0 began, 1 moved, 2 ended, 3 cancelled; (x, y) in view points; `time` in
/// seconds on a steady clock.
///
/// # Safety
/// As `on_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_touch(h: *mut OnHost, id: u64, phase: i32, x: f32, y: f32, time: f64) {
    let Some(h) = (unsafe { h.as_mut() }) else { return };
    let phase = match phase {
        0 => TouchPhase::Started,
        1 => TouchPhase::Moved,
        2 => TouchPhase::Ended,
        _ => TouchPhase::Cancelled,
    };
    h.host.runtime_mut().touch(id, phase, x, y, time);
}

/// Text from the keyboard. A newline is the Return key.
///
/// # Safety
/// As `on_frame`; `text` is NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_text(h: *mut OnHost, text: *const c_char) {
    let Some(h) = (unsafe { h.as_mut() }) else { return };
    if text.is_null() {
        return;
    }
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned();
    let rt = h.host.runtime_mut();
    for (i, part) in text.split('\n').enumerate() {
        if i > 0 {
            rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        }
        if !part.is_empty() {
            rt.ime(Ime::Commit(part.to_owned()));
        }
    }
}

/// The keyboard's delete key.
///
/// # Safety
/// As `on_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_backspace(h: *mut OnHost) {
    if let Some(h) = unsafe { h.as_mut() } {
        h.host.runtime_mut().key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
    }
}

/// The fold of a partly folded iPhone Duo, in view points, or `active = false` when nothing
/// divides the screen. The notebook's two pages meet at it.
///
/// # Safety
/// As `on_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_fold(h: *mut OnHost, x: f32, y: f32, width: f32, height: f32, active: bool) {
    let Some(h) = (unsafe { h.as_mut() }) else { return };
    let fold = active.then_some([x, y, width, height]);
    let rt = h.host.runtime_mut();
    if rt.state.fold != fold {
        rt.state.fold = fold;
        rt.invalidate();
    }
}

/// The home indicator's strip at the bottom, in points, while no keyboard covers it.
///
/// # Safety
/// As `on_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_inset_bottom(h: *mut OnHost, bottom: f32) {
    let Some(h) = (unsafe { h.as_mut() }) else { return };
    let rt = h.host.runtime_mut();
    if (rt.state.inset_bottom - bottom).abs() > 0.5 {
        rt.state.inset_bottom = bottom;
        rt.invalidate();
    }
}

/// Ends the app and frees the host.
///
/// # Safety
/// `h` comes from `on_create` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_destroy(h: *mut OnHost) {
    if !h.is_null() {
        drop(unsafe { Box::from_raw(h) });
    }
}
