//! Linux: the window has no frame (the app draws its own titlebar), so nothing at its edges
//! would resize it. Input-only X11 windows along its edges and in its corners take a press
//! there and hand it to the window manager as a resize (`_NET_WM_MOVERESIZE`), with the
//! cursors a frame shows. They step aside while the window is maximized or full screen.
//!
//! They run on a thread of their own with their own X connection: the presses on them are
//! that connection's, so it can let go of the pointer for the window manager to take it.

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, StackMode, Window, WindowClass,
};
use x11rb::rust_connection::ReplyOrIdError;
use x11rb::xcb_ffi::XCBConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT};

use rune::raw_window_handle::RawWindowHandle;

/// How far into the window an edge takes presses, and how far along each side a corner does,
/// in pixels.
const EDGE: i32 = 6;
const CORNER: i32 = 16;

/// The edges and corners: `_NET_WM_MOVERESIZE`'s direction for each, and its glyph in the X
/// cursor font, clockwise from the top-left corner.
const SIDES: [(u32, u16); 8] = [(0, 134), (1, 138), (2, 136), (3, 96), (4, 14), (5, 16), (6, 12), (7, 70)];

/// Starts the edges around `window`.
pub fn start(window: &rune::WindowHandle) {
    let parent = window.with_raw_handle(|handle| match handle.as_raw() {
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).ok(),
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        _ => None,
    });
    let Ok(Some(parent)) = parent else { return };
    let _ = std::thread::Builder::new().name("window-edges".into()).spawn(move || {
        if let Err(error) = run(parent) {
            eprintln!("rune-browser: the window's edges won't resize it: {error}");
        }
    });
}

fn atom(x: &XCBConnection, name: &[u8]) -> Result<Atom, Box<dyn std::error::Error>> {
    Ok(x.intern_atom(false, name)?.reply()?.atom)
}

fn run(parent: Window) -> Result<(), Box<dyn std::error::Error>> {
    let (x, screen) = XCBConnection::connect(None)?;
    let root = x.setup().roots[screen].root;
    let moveresize = atom(&x, b"_NET_WM_MOVERESIZE")?;
    let state = atom(&x, b"_NET_WM_STATE")?;
    let maximized = [atom(&x, b"_NET_WM_STATE_MAXIMIZED_VERT")?, atom(&x, b"_NET_WM_STATE_MAXIMIZED_HORZ")?];
    let fullscreen = atom(&x, b"_NET_WM_STATE_FULLSCREEN")?;

    let font = x.generate_id()?;
    x.open_font(font, b"cursor")?;
    let mut edges = Vec::with_capacity(SIDES.len());
    for (direction, glyph) in SIDES {
        let cursor = x.generate_id()?;
        x.create_glyph_cursor(cursor, font, font, glyph, glyph + 1, 0, 0, 0, u16::MAX, u16::MAX, u16::MAX)?;
        let edge = x.generate_id()?;
        let aux = CreateWindowAux::new().event_mask(EventMask::BUTTON_PRESS).cursor(cursor);
        x.create_window(COPY_DEPTH_FROM_PARENT, edge, parent, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, COPY_FROM_PARENT, &aux)?;
        edges.push((edge, direction));
    }
    x.change_window_attributes(parent, &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE))?;

    // Places the edges for the window's size, or takes them away while it fills the screen.
    let place = |x: &XCBConnection| -> Result<(), ReplyOrIdError> {
        let geometry = x.get_geometry(parent)?.reply()?;
        let states = x.get_property(false, parent, state, AtomEnum::ATOM, 0, 64)?.reply()?;
        let states: Vec<Atom> = states.value32().map(Iterator::collect).unwrap_or_default();
        let fills = states.contains(&fullscreen) || maximized.iter().all(|atom| states.contains(atom));
        let (w, h) = (i32::from(geometry.width), i32::from(geometry.height));
        for &(edge, direction) in &edges {
            if fills {
                x.unmap_window(edge)?;
                continue;
            }
            let [left, top, width, height] = match direction {
                0 => [0, 0, CORNER, CORNER],
                1 => [CORNER, 0, w - 2 * CORNER, EDGE],
                2 => [w - CORNER, 0, CORNER, CORNER],
                3 => [w - EDGE, CORNER, EDGE, h - 2 * CORNER],
                4 => [w - CORNER, h - CORNER, CORNER, CORNER],
                5 => [CORNER, h - EDGE, w - 2 * CORNER, EDGE],
                6 => [0, h - CORNER, CORNER, CORNER],
                _ => [0, CORNER, EDGE, h - 2 * CORNER],
            };
            let aux = ConfigureWindowAux::new().x(left).y(top).width(width.max(1) as u32).height(height.max(1) as u32).stack_mode(StackMode::ABOVE);
            x.configure_window(edge, &aux)?;
            x.map_window(edge)?;
        }
        x.flush()?;
        Ok(())
    };
    place(&x)?;

    loop {
        match x.wait_for_event()? {
            Event::ConfigureNotify(event) if event.window == parent => place(&x)?,
            Event::PropertyNotify(event) if event.window == parent && event.atom == state => place(&x)?,
            Event::DestroyNotify(event) if event.window == parent => return Ok(()),
            Event::ButtonPress(event) if event.detail == 1 => {
                let Some(&(_, direction)) = edges.iter().find(|(edge, _)| *edge == event.event) else { continue };
                // The window manager takes the pointer for the resize: the press holds it until
                // it is let go.
                x.ungrab_pointer(event.time)?;
                let message = ClientMessageEvent::new(32, parent, moveresize, [event.root_x as u32, event.root_y as u32, direction, 1, 1]);
                x.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, message)?;
                x.flush()?;
            }
            _ => {}
        }
    }
}
