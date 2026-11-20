//! The Phosphor icon font, used for the category icons.
//!
//! Icon fonts draw one glyph per character on a fixed advance, so every icon
//! occupies the same width and the text after them can be placed with a tab
//! stop. Symbol images cannot do that: each one advances by its own glyph's
//! width whatever size it is given.

use crate::objc::{self, Obj};

const FONT_FILE: &str = "Phosphor";

pub fn load() -> Option<()> {
    unsafe {
        let bundle = objc::send(objc::class("NSBundle"), "mainBundle");
        let name = objc::nsstring(FONT_FILE);
        let path = objc::send_obj_obj(bundle, "pathForResource:ofType:", name, objc::nsstring("ttf"));
        if path.is_null() {
            return None;
        }
        let url = objc::send_obj(
            objc::class("NSURL"),
            "fileURLWithPath:",
            path,
        );
        if url.is_null() {
            return None;
        }
        let registered = ct_font_register(url);
        if !registered && font(13.0).is_none() {
            return None;
        }
        Some(())
    }
}

unsafe fn ct_font_register(url: Obj) -> bool {
    #[link(name = "CoreText", kind = "framework")]
    extern "C" {
        fn CTFontManagerRegisterFontsForURL(url: Obj, scope: i32, error: *mut Obj) -> bool;
    }
    // kCTFontManagerScopeProcess
    let mut error: Obj = std::ptr::null_mut();
    CTFontManagerRegisterFontsForURL(url, 1, &mut error)
}

/// The icon font at a size, if it is available. Cached: a row build asks for
/// it once per row and the answer never changes.
pub fn font(size: f64) -> Option<Obj> {
    thread_local! {
        static FONTS: std::cell::RefCell<std::collections::HashMap<u64, usize>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let key = size.to_bits();
    if let Some(pointer) = FONTS.with(|cache| cache.borrow().get(&key).copied()) {
        return if pointer == 0 { None } else { Some(pointer as Obj) };
    }
    let resolved = unsafe {
        let mut found = std::ptr::null_mut();
        for name in ["Phosphor", "Phosphor-Regular"] {
            let font = objc::send_obj_float(
                objc::class("NSFont"),
                "fontWithName:size:",
                objc::nsstring(name),
                size,
            );
            if !font.is_null() {
                found = font;
                break;
            }
        }
        found
    };
    FONTS.with(|cache| {
        cache.borrow_mut().insert(key, resolved as usize);
    });
    if resolved.is_null() { None } else { Some(resolved) }
}

impl crate::model::Category {
    /// Phosphor codepoint for this category.
    pub fn icon(self) -> char {
        match self {
            crate::model::Category::Temperature => '\u{e5c6}',
            crate::model::Category::Voltage => '\u{e2de}',
            crate::model::Category::Fan => '\u{e9f2}',
            crate::model::Category::Memory => '\u{e9c4}',
            crate::model::Category::Processor => '\u{e610}',
            crate::model::Category::System => '\u{e628}',
            crate::model::Category::Network => '\u{edde}',
            crate::model::Category::Storage => '\u{e2a0}',
            crate::model::Category::Battery => '\u{e0c0}',
        }
    }
}
