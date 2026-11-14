//! The native views Chromium draws into, and where they sit in the Rune window.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

/// A hidden, layer-backed view over the window's content view. Every tab's browser view goes
/// inside it, so one frame and one corner radius place them all.
pub fn container(parent: &NSView, mtm: MainThreadMarker) -> Retained<NSView> {
    let view = NSView::initWithFrame(mtm.alloc(), NSRect::ZERO);
    view.setWantsLayer(true);
    view.setHidden(true);
    parent.addSubview(&view);
    view
}

/// Moves the container to `rect` (window points from the top-left, as Rune lays out), less
/// the `cover_top` points at its top the app draws on, and rounds its corners: all four, or
/// only the bottom two when something sits flush above it. `shown` is false while the tab on
/// screen has no page.
pub fn place(container: &NSView, parent: &NSView, [x, y, w, h]: [f32; 4], radius: f32, square_top: bool, cover_top: f32, shown: bool) {
    let cover = f64::from(cover_top.clamp(0.0, h.max(0.0)));
    let (x, y, w, h) = (f64::from(x), f64::from(y) + cover, f64::from(w), f64::from(h) - cover);
    let y = if parent.isFlipped() { y } else { parent.bounds().size.height - y - h };
    container.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
    // CACornerMask: min-x min-y, max-x min-y, min-x max-y, max-x max-y. A layer's y runs up
    // unless its view is flipped, so "min y" is the bottom edge there.
    let (bottom, top): (usize, usize) = if container.isFlipped() { (4 | 8, 1 | 2) } else { (1 | 2, 4 | 8) };
    let corners = if square_top || cover > 0.0 { bottom } else { bottom | top };
    // SAFETY: a layer-backed view's layer is a CALayer, which has these properties.
    unsafe {
        let layer: *mut AnyObject = msg_send![container, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setCornerRadius: f64::from(radius)];
            let _: () = msg_send![layer, setMaskedCorners: corners];
            let _: () = msg_send![layer, setMasksToBounds: true];
        }
    }
    container.setHidden(!shown || w < 1.0 || h < 1.0);
}

/// Sizes a browser's view to the whole page area, held to the container's bottom edge: when
/// the container is cut short at the top, the page keeps its size and is clipped there.
pub fn fill(browser_view: &NSView, (width, height): (f64, f64)) {
    // SAFETY: the superview is the live container, used on the main thread.
    let container = unsafe { browser_view.superview() };
    let container_height = container.as_ref().map_or(height, |container| container.bounds().size.height);
    let flipped = container.as_ref().is_some_and(|container| container.isFlipped());
    let y = if flipped { container_height - height } else { 0.0 };
    browser_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);
    browser_view.setFrame(NSRect::new(NSPoint::new(0.0, y), NSSize::new(width.max(1.0), height.max(1.0))));
}

/// Gives `view` the keyboard in its window.
pub fn focus(view: &NSView) {
    if let Some(window) = view.window() {
        window.makeFirstResponder(Some(view));
    }
}

/// Chromium's own dialogs (adding an extension, a passkey prompt) are windows it makes
/// without the app's say. Embedded like this it leaves them with an opaque title bar over
/// their top edge and places them by a window it doesn't have. Each one is given a clear
/// title bar, tied to `window` so it moves and stays with it, and centred near the top of
/// the page area, `page` (window points from the top-left). Called every turn of the loop:
/// a dialog is corrected as it appears and again whenever Chromium moves or resizes it.
pub fn tend_dialogs(window: &NSWindow, [x, y, w, h]: [f32; 4], reveal: bool) {
    const TITLED: usize = 1;
    const FRAME_BITS: usize = 0xf;
    const FULL_SIZE_CONTENT: usize = 1 << 15;
    // SAFETY: plain AppKit calls on the main thread, on windows the app owns.
    unsafe {
        let Some(class) = AnyClass::get(c"NSApplication") else { return };
        let app: *mut AnyObject = msg_send![class, sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        let home = window.frame();
        for index in 0..count {
            let dialog: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let visible: bool = msg_send![dialog, isVisible];
            let mask: usize = msg_send![dialog, styleMask];
            // Chromium's own windows that aren't ordinary ones: no close button, no resizing.
            if mask & FRAME_BITS > TITLED || (*dialog).class().name().to_bytes() != b"NativeWidgetMacNSWindow" {
                continue;
            }
            // A dialog the hidden extensions page asked for is as hidden as that page's
            // window: `reveal` says one is expected, and it is brought out over this window.
            if !visible {
                if !reveal || mask & FRAME_BITS != TITLED {
                    continue;
                }
                let _: () = msg_send![dialog, setAlphaValue: 1.0f64];
                let _: () = msg_send![window, addChildWindow: dialog, ordered: 1isize];
                let _: () = msg_send![dialog, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
            }
            // Without a parent it can open behind the browser window and never be seen.
            let parent: *mut AnyObject = msg_send![dialog, parentWindow];
            if parent.is_null() {
                let _: () = msg_send![window, addChildWindow: dialog, ordered: 1isize];
            }
            // The rest is for a titled dialog. A borderless one (a passkey prompt, a tooltip)
            // is drawn whole and placed by Chromium.
            if mask & FRAME_BITS != TITLED || mask & FULL_SIZE_CONTENT == 0 {
                continue;
            }
            let clear: bool = msg_send![dialog, titlebarAppearsTransparent];
            if !clear {
                let _: () = msg_send![dialog, setTitlebarAppearsTransparent: true];
                let _: () = msg_send![dialog, setTitleVisibility: 1isize];
            }
            let frame: NSRect = msg_send![dialog, frame];
            let drop = ((f64::from(h) - frame.size.height) / 2.0).clamp(0.0, 72.0);
            let left = home.origin.x + f64::from(x) + (f64::from(w) - frame.size.width) / 2.0;
            let bottom = home.origin.y + home.size.height - f64::from(y) - drop - frame.size.height;
            if (frame.origin.x - left).abs() > 0.5 || (frame.origin.y - bottom).abs() > 0.5 {
                let _: () = msg_send![dialog, setFrameOrigin: NSPoint::new(left.round(), bottom.round())];
            }
        }
    }
}
