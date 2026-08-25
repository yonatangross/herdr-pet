//! macOS mouse-drag helper: a session-level CGEventTap on its own thread.
//! While the configured modifiers are held and the left button is down, the
//! mouse events are swallowed (the terminal sees no click/selection) and the
//! drag deltas are sent to the daemon. Needs Accessibility permission.
#![cfg(target_os = "macos")]

use crate::daemon::Msg;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::mach_port::{CFMachPort, CFMachPortRef};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_foundation::string::CFString;
use std::ffi::c_void;
use std::sync::mpsc::Sender;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type Callback = unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: Callback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    // CoreFoundation `Boolean` is an unsigned char, not a C99 `_Bool`.
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> u8;
}

const SESSION_EVENT_TAP: u32 = 1;
const HEAD_INSERT_EVENT_TAP: u32 = 0;
const TAP_OPTION_DEFAULT: u32 = 0;
const LEFT_MOUSE_DOWN: u32 = 1;
const LEFT_MOUSE_UP: u32 = 2;
const LEFT_MOUSE_DRAGGED: u32 = 6;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;
const FLAG_ALL: u64 = FLAG_SHIFT | FLAG_CONTROL | FLAG_ALTERNATE | FLAG_COMMAND;

fn parse_modifiers(spec: &str) -> Result<u64, String> {
    let mut flags = 0;
    for part in spec.split('+') {
        flags |= match part.trim().to_ascii_lowercase().as_str() {
            "control" | "ctrl" => FLAG_CONTROL,
            "option" | "alt" => FLAG_ALTERNATE,
            "command" | "cmd" => FLAG_COMMAND,
            "shift" => FLAG_SHIFT,
            other => return Err(format!("unknown modifier {other:?}")),
        };
    }
    if flags == 0 {
        return Err("no modifiers given".into());
    }
    Ok(flags)
}

struct State {
    want: u64,
    tx: Sender<Msg>,
    dragging: bool,
    last: CGPoint,
    tap: CFMachPortRef,
}

unsafe extern "C" fn on_event(_proxy: CGEventTapProxy, etype: u32, event: CGEventRef, user_info: *mut c_void) -> CGEventRef {
    let state = &mut *(user_info as *mut State);
    if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER_INPUT {
        // Events were missed while disabled, possibly the mouse-up that ends a
        // drag; never carry a gesture across the gap or plain clicks get swallowed.
        if state.dragging {
            state.dragging = false;
            let _ = state.tx.send(Msg::DragEnd);
        }
        CGEventTapEnable(state.tap, true);
        return event;
    }
    let mods = CGEventGetFlags(event) & FLAG_ALL;
    match etype {
        LEFT_MOUSE_DOWN if mods == state.want => {
            state.dragging = true;
            state.last = CGEventGetLocation(event);
            let _ = state.tx.send(Msg::DragStart);
            std::ptr::null_mut()
        }
        LEFT_MOUSE_DRAGGED if state.dragging => {
            let loc = CGEventGetLocation(event);
            let _ = state.tx.send(Msg::Drag { dx: loc.x - state.last.x, dy: loc.y - state.last.y });
            state.last = loc;
            std::ptr::null_mut()
        }
        LEFT_MOUSE_UP if state.dragging => {
            state.dragging = false;
            let _ = state.tx.send(Msg::DragEnd);
            std::ptr::null_mut()
        }
        _ => event,
    }
}

fn is_trusted(prompt: bool) -> bool {
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let opts = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::from(prompt).as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef()) != 0 }
}

/// Start the tap thread. Errors are reported through `Msg::DragInfo`; the
/// thread runs a CFRunLoop for the life of the process.
pub fn start(modifiers: &str, tx: Sender<Msg>) {
    let want = match parse_modifiers(modifiers) {
        Ok(w) => w,
        Err(e) => {
            let _ = tx.send(Msg::DragInfo(format!("drag disabled: {e}")));
            return;
        }
    };
    let spec = modifiers.to_owned();
    std::thread::Builder::new()
        .name("petdrag".into())
        .spawn(move || {
            if !is_trusted(true) {
                let _ = tx.send(Msg::DragInfo(
                    "Accessibility permission missing — allow the terminal app under System Settings → Privacy & Security → Accessibility, then restart the pet".into(),
                ));
            }
            let state = Box::into_raw(Box::new(State {
                want,
                tx: tx.clone(),
                dragging: false,
                last: CGPoint::default(),
                tap: std::ptr::null_mut(),
            }));
            let mask = (1u64 << LEFT_MOUSE_DOWN) | (1 << LEFT_MOUSE_UP) | (1 << LEFT_MOUSE_DRAGGED);
            let port = unsafe {
                CGEventTapCreate(SESSION_EVENT_TAP, HEAD_INSERT_EVENT_TAP, TAP_OPTION_DEFAULT, mask, on_event, state as *mut c_void)
            };
            if port.is_null() {
                let _ = tx.send(Msg::DragInfo("could not create event tap (Accessibility permission missing?); drag disabled".into()));
                unsafe { drop(Box::from_raw(state)) };
                return;
            }
            unsafe { (*state).tap = port };
            let port = unsafe { CFMachPort::wrap_under_create_rule(port) };
            let Ok(source) = port.create_runloop_source(0) else {
                let _ = tx.send(Msg::DragInfo("could not create run loop source; drag disabled".into()));
                unsafe { drop(Box::from_raw(state)) };
                return;
            };
            unsafe {
                CFRunLoop::get_current().add_source(&source, kCFRunLoopCommonModes);
                CGEventTapEnable(port.as_concrete_TypeRef(), true);
            }
            let _ = tx.send(Msg::DragInfo(format!("hold {spec} and drag anywhere to move the pet")));
            CFRunLoop::run_current();
            let _ = tx.send(Msg::DragInfo("event tap run loop ended; drag disabled until restart".into()));
        })
        .expect("spawn drag thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifier_combos() {
        assert_eq!(parse_modifiers("control+option"), Ok(FLAG_CONTROL | FLAG_ALTERNATE));
        assert_eq!(parse_modifiers("cmd+shift"), Ok(FLAG_COMMAND | FLAG_SHIFT));
        assert!(parse_modifiers("hyper").is_err());
        assert!(parse_modifiers("").is_err());
    }
}
