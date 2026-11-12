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
    system_dark: bool,
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
    /// Present the system document picker after this frame.
    pub pick_files: bool,
    /// The app appearance, including an explicit setting.
    pub dark: bool,
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
    // The simulator uses the same sample scenes as headless renders.
    #[cfg(target_abi = "sim")]
    let state = {
        let mut state = state;
        let args: Vec<String> = std::env::args().collect();
        let scene = args.windows(2).find(|pair| pair[0] == "--scene").map(|pair| pair[1].as_str());
        if let Some((_, apply)) = spec.scenes.iter().find(|(name, _)| Some(*name) == scene) {
            apply(&mut state);
        }
        state
    };
    let ctx = Arc::new(WakeCtx(ctx));
    let waker: rune::Waker = Arc::new(move || wake(ctx.0));
    match unsafe { LayerHost::from_core_animation_layer(layer, &spec, state, (width, height), scale, dark, waker) } {
        Ok(host) => Box::into_raw(Box::new(OnHost { host, system_dark: dark })),
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
    let Some(h) = (unsafe { h.as_mut() }) else { return OnFrame { animating: false, wants_keyboard: false, wake_after: -1.0, pick_files: false, dark: false } };
    let f = h.host.frame();
    let pick_files = h.host.runtime_mut().state.open.as_mut().and_then(|o| o.add.as_mut()).is_some_and(|a| std::mem::take(&mut a.pick_files));
    let rt = h.host.runtime();
    let dark = match rt.state.settings.appearance.as_str() {
        "dark" => true,
        "light" => false,
        _ => h.system_dark,
    };
    OnFrame { animating: f.animating, wants_keyboard: rt.ime_area.is_some(), wake_after: f.wake_after_secs.unwrap_or(-1.0), pick_files, dark }
}

/// The containing view's current system appearance.
///
/// # Safety
/// As `on_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_appearance(h: *mut OnHost, dark: bool) {
    if let Some(h) = unsafe { h.as_mut() } {
        h.system_dark = dark;
        h.host.runtime_mut().set_system_dark(dark);
    }
}

/// A document copied into the app's temporary directory, or a picker error.
///
/// # Safety
/// As `on_frame`; strings must be NUL-terminated UTF-8 when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_file(h: *mut OnHost, path: *const c_char, error: *const c_char) {
    let Some(h) = (unsafe { h.as_mut() }) else { return };
    let rt = h.host.runtime_mut();
    let Some(a) = rt.state.open.as_mut().and_then(|o| o.add.as_mut()) else { return };
    if !error.is_null() {
        a.error = Some(unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned());
    } else if !path.is_null() {
        let path = std::path::PathBuf::from(unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned());
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        if crate::state::accepted().contains(&extension.as_str()) {
            a.add_files(&[path]);
        } else {
            a.error = Some(format!("Unsupported file type: {}", path.file_name().unwrap_or_default().to_string_lossy()));
        }
    }
    rt.invalidate();
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

/// Dismisses the keyboard while retaining the field's text.
///
/// # Safety
/// As `on_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_dismiss_keyboard(h: *mut OnHost) {
    if let Some(h) = unsafe { h.as_mut() } {
        h.host.runtime_mut().update(|_, tx| tx.blur());
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

/// The home indicator's strip at the bottom, in points (0 while the keyboard covers it), and
/// whether the on-screen keyboard is up.
///
/// # Safety
/// As `on_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_inset_bottom(h: *mut OnHost, bottom: f32, keyboard: bool) {
    let Some(h) = (unsafe { h.as_mut() }) else { return };
    let rt = h.host.runtime_mut();
    if (rt.state.inset_bottom - bottom).abs() > 0.5 || rt.state.keyboard_up != keyboard {
        rt.state.inset_bottom = bottom;
        rt.state.keyboard_up = keyboard;
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
