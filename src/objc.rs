//! Minimal hand-written bindings to the Objective-C runtime and AppKit.
//!
//! Deliberately not a binding crate: the app needs a dozen classes and this
//! keeps the build dependency-free, at the cost of every message crossing an
//! `objc_msgSend` transmute whose signature has to be right.

use std::ffi::{c_char, c_void, CString};
use std::mem::transmute;

pub type Obj = *mut c_void;
pub type Sel = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NSSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NSPoint {
    pub x: f64,
    pub y: f64,
}

#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Obj;
    fn sel_registerName(name: *const c_char) -> Sel;
    pub fn objc_allocateClassPair(superclass: Obj, name: *const c_char, extra_bytes: usize) -> Obj;
    pub fn objc_registerClassPair(cls: Obj);
    pub fn class_addMethod(cls: Obj, sel: Sel, imp: *const c_void, types: *const c_char) -> bool;
    pub fn object_getClass(obj: Obj) -> Obj;
    fn objc_msgSend();
}

#[link(name = "Foundation", kind = "framework")]
extern "C" {
    pub static NSRunLoopCommonModes: Obj;
    pub static NSEventTrackingRunLoopMode: Obj;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

const NS_ASCII_STRING_ENCODING: usize = 1;

fn cstring(value: &str) -> CString {
    CString::new(value).unwrap_or_else(|_| CString::new("").unwrap())
}

pub unsafe fn class(name: &str) -> Obj {
    objc_getClass(cstring(name).as_ptr())
}

pub unsafe fn selector(name: &str) -> Sel {
    sel_registerName(cstring(name).as_ptr())
}

/// Sends a message that takes no arguments and returns an object.
pub unsafe fn send(obj: Obj, sel: &str) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector(sel))
}

pub unsafe fn send_obj(obj: Obj, sel: &str, arg: Obj) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, Obj) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

pub unsafe fn send_int(obj: Obj, sel: &str, arg: isize) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, isize) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

pub unsafe fn send_float(obj: Obj, sel: &str, arg: f64) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, f64) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

pub unsafe fn send_size(obj: Obj, sel: &str) -> NSSize {
    let function: unsafe extern "C" fn(Obj, Sel) -> NSSize = transmute(objc_msgSend as *const ());
    function(obj, selector(sel))
}

pub unsafe fn send_void(obj: Obj, sel: &str) {
    let function: unsafe extern "C" fn(Obj, Sel) = transmute(objc_msgSend as *const ());
    function(obj, selector(sel))
}

pub unsafe fn send_void_obj(obj: Obj, sel: &str, arg: Obj) {
    let function: unsafe extern "C" fn(Obj, Sel, Obj) = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

pub unsafe fn send_void_int(obj: Obj, sel: &str, arg: isize) {
    let function: unsafe extern "C" fn(Obj, Sel, isize) = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

pub unsafe fn send_void_float(obj: Obj, sel: &str, arg: f64) {
    let function: unsafe extern "C" fn(Obj, Sel, f64) = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

pub unsafe fn send_void_obj_int(obj: Obj, sel: &str, a: Obj, b: isize) {
    let function: unsafe extern "C" fn(Obj, Sel, Obj, isize) =
        transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b)
}

pub unsafe fn send_int_result(obj: Obj, sel: &str) -> isize {
    let function: unsafe extern "C" fn(Obj, Sel) -> isize = transmute(objc_msgSend as *const ());
    function(obj, selector(sel))
}

pub unsafe fn send_float_result(obj: Obj, sel: &str) -> f64 {
    let function: unsafe extern "C" fn(Obj, Sel) -> f64 = transmute(objc_msgSend as *const ());
    function(obj, selector(sel))
}

pub unsafe fn send_bool_result(obj: Obj, sel: &str) -> bool {
    let function: unsafe extern "C" fn(Obj, Sel) -> bool = transmute(objc_msgSend as *const ());
    function(obj, selector(sel))
}

/// Allocates an instance of `name`.
pub unsafe fn alloc(name: &str) -> Obj {
    send(class(name), "alloc")
}

pub unsafe fn nsstring(value: &str) -> Obj {
    let class = class("NSString");
    let c = cstring(value);
    let function: unsafe extern "C" fn(Obj, Sel, *const c_char, usize) -> Obj =
        transmute(objc_msgSend as *const ());
    function(
        class,
        selector("stringWithUTF8String:"),
        c.as_ptr(),
        NS_ASCII_STRING_ENCODING,
    )
}

pub unsafe fn nsnumber_int(value: isize) -> Obj {
    send_int(class("NSNumber"), "numberWithInteger:", value)
}

pub fn msg_send_ptr() -> *const () {
    objc_msgSend as *const ()
}

pub unsafe fn send_void_obj_obj(obj: Obj, sel: &str, a: Obj, b: Obj) {
    let function: unsafe extern "C" fn(Obj, Sel, Obj, Obj) = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b);
}

pub unsafe fn send_float_obj_result(obj: Obj, sel: &str, arg: Obj) -> f64 {
    let function: unsafe extern "C" fn(Obj, Sel, Obj) -> f64 = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

pub unsafe fn send_int_obj_result(obj: Obj, sel: &str, arg: Obj) -> isize {
    let function: unsafe extern "C" fn(Obj, Sel, Obj) -> isize = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

/// `-addAttribute:value:range:` (range is an NSRange, two uintptr-sized words).
pub unsafe fn send_obj_obj(obj: Obj, sel: &str, a: Obj, b: Obj) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, Obj, Obj) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b)
}

pub unsafe fn send_obj_sel_obj(obj: Obj, sel: &str, a: Obj, action: Sel, b: Obj) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, Obj, Sel, Obj) -> Obj =
        transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, action, b)
}

pub unsafe fn send_obj_two_floats(obj: Obj, sel: &str, a: f64, b: f64) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, f64, f64) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b)
}

pub unsafe fn send_size_obj(obj: Obj, sel: &str, a: Obj) -> NSSize {
    let function: unsafe extern "C" fn(Obj, Sel, Obj) -> NSSize = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a)
}

/// `-[NSTextTab initWithTextAlignment:location:options:]`. NSTextAlignmentRight
/// is 1 — 2 is centre, which staggers the column instead of aligning it.
pub unsafe fn send_tab(location: f64) -> Obj {
    let options = send(class("NSDictionary"), "dictionary");
    let function: unsafe extern "C" fn(Obj, Sel, isize, f64, Obj) -> Obj =
        transmute(objc_msgSend as *const ());
    function(
        alloc("NSTextTab"),
        selector("initWithTextAlignment:location:options:"),
        0,
        location,
        options,
    )
}

pub fn system_font_size() -> f64 {
    13.0
}

pub unsafe fn send_void_two_floats(obj: Obj, sel: &str, a: f64, b: f64) {
    let function: unsafe extern "C" fn(Obj, Sel, f64, f64) = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b);
}

/// `+timerWithTimeInterval:target:selector:userInfo:repeats:`
type TimerFactory =
    unsafe extern "C" fn(Obj, Sel, f64, Obj, Sel, Obj, bool) -> Obj;

pub unsafe fn send_timer(
    obj: Obj,
    sel: &str,
    interval: f64,
    target: Obj,
    action: Sel,
    user_info: Obj,
    repeats: bool,
) -> Obj {
    let function: TimerFactory = transmute(msg_send_ptr());
    function(obj, selector(sel), interval, target, action, user_info, repeats)
}

pub unsafe fn send_attr_range(obj: Obj, sel: &str, name: Obj, value: Obj, loc: usize, len: usize) {
    let function: unsafe extern "C" fn(Obj, Sel, Obj, Obj, usize, usize) =
        transmute(objc_msgSend as *const ());
    function(obj, selector(sel), name, value, loc, len);
}

pub unsafe fn retain(obj: Obj) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector("retain"))
}

pub unsafe fn release(obj: Obj) {
    let function: unsafe extern "C" fn(Obj, Sel) = transmute(objc_msgSend as *const ());
    function(obj, selector("release"));
}

pub unsafe fn send_bool_obj_result(obj: Obj, sel: &str, a: Obj, b: bool) -> bool {
    let function: unsafe extern "C" fn(Obj, Sel, Obj, bool) -> bool = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b)
}

pub unsafe fn send_obj_int_obj(obj: Obj, sel: &str, a: isize, b: Obj) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, isize, Obj) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b)
}

pub unsafe fn send_bool_error(obj: Obj, sel: &str, error: *mut Obj) -> bool {
    let function: unsafe extern "C" fn(Obj, Sel, *mut Obj) -> bool = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), error)
}

pub unsafe fn nsstring_to_string(value: Obj) -> String {
    if value.is_null() {
        return String::new();
    }
    let utf8 = send(value, "UTF8String") as *const std::ffi::c_char;
    if utf8.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()
}

#[repr(C)]
pub struct CgRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub unsafe fn send_void_rect(obj: Obj, sel: &str, rect: &CgRect) {
    let function: unsafe extern "C" fn(Obj, Sel, *const CgRect) = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), rect);
}

pub unsafe fn send_void_rect_rect_int_float(
    obj: Obj,
    sel: &str,
    a: *const CgRect,
    b: *const CgRect,
    op: isize,
    fraction: f64,
) {
    let function: unsafe extern "C" fn(Obj, Sel, *const CgRect, *const CgRect, isize, f64) =
        transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b, op, fraction);
}

pub unsafe fn send_obj_size_int(obj: Obj, sel: &str, width: f64, height: f64, flag: isize) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, f64, f64, isize) -> Obj =
        transmute(objc_msgSend as *const ());
    function(obj, selector(sel), width, height, flag)
}

pub unsafe fn send_void_range_point(obj: Obj, sel: &str, location: usize, length: usize, x: f64, y: f64) {
    let function: unsafe extern "C" fn(Obj, Sel, usize, usize, f64, f64) =
        transmute(objc_msgSend as *const ());
    function(obj, selector(sel), location, length, x, y);
}

pub unsafe fn send_point_int(obj: Obj, sel: &str, index: usize) -> (f64, f64) {
    let function: unsafe extern "C" fn(Obj, Sel, usize) -> (f64, f64) =
        transmute(objc_msgSend as *const ());
    function(obj, selector(sel), index)
}

pub unsafe fn send_int_obj_result2(obj: Obj, sel: &str, arg: usize) -> usize {
    let function: unsafe extern "C" fn(Obj, Sel, usize) -> usize = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), arg)
}

pub unsafe fn bitmap_rep(width: isize, height: isize) -> Obj {
    let function: unsafe extern "C" fn(
        Obj,
        Sel,
        *mut u8,
        isize,
        isize,
        isize,
        isize,
        bool,
        bool,
        Obj,
        isize,
        isize,
    ) -> Obj = transmute(objc_msgSend as *const ());
    function(
        alloc("NSBitmapImageRep"),
        selector("initWithBitmapDataPlanes:pixelsWide:pixelsHigh:bitsPerSample:samplesPerPixel:hasAlpha:isPlanar:colorSpaceName:bytesPerRow:bitsPerPixel:"),
        std::ptr::null_mut(),
        width,
        height,
        8,
        4,
        true,
        false,
        nsstring("NSDeviceRGBColorSpace"),
        0,
        0,
    )
}

pub unsafe fn graphics_context_for(bitmap: Obj) -> Obj {
    send_obj(class("NSGraphicsContext"), "graphicsContextWithBitmapImageRep:", bitmap)
}

pub unsafe fn set_current_graphics_context(context: Obj) {
    send_void_obj(class("NSGraphicsContext"), "setCurrentContext:", context);
}

pub unsafe fn send_obj_float(obj: Obj, sel: &str, a: Obj, b: f64) -> Obj {
    let function: unsafe extern "C" fn(Obj, Sel, Obj, f64) -> Obj = transmute(objc_msgSend as *const ());
    function(obj, selector(sel), a, b)
}
