//! The browser's colours, light and dark, as designed in Paper ("Rune Browser").

use rune::Color;

/// `0xRRGGBB` in sRGB.
pub(super) fn rgb(hex: u32) -> Color {
    rgba(hex, 1.0)
}

pub(super) fn rgba(hex: u32, alpha: f32) -> Color {
    // Rune's colours are linear-light.
    let channel = |shift: u32| {
        let encoded = ((hex >> shift) & 0xff) as f32 / 255.0;
        if encoded <= 0.04045 { encoded / 12.92 } else { ((encoded + 0.055) / 1.055).powf(2.4) }
    };
    Color { r: channel(16), g: channel(8), b: channel(0), a: alpha }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// The window behind everything.
    pub window: Color,
    /// The address field and pinned tiles.
    pub field: Color,
    /// The panel the page sits on.
    pub page: Color,
    /// The selected tab's pill.
    pub tab_active: Color,
    pub tab_shadow: Color,
    pub tab_ring: Color,
    pub page_ring: Color,
    pub page_shadow: Color,
    pub text: Color,
    /// The selected tab's title.
    pub text_strong: Color,
    /// Unselected tab titles in the sidebar.
    pub text_soft: Color,
    /// Unselected tab titles in the top strip.
    pub text_strip: Color,
    pub muted: Color,
    /// The path after the site in the address field.
    pub faint: Color,
    pub icon: Color,
    pub divider: Color,
    pub hover: Color,
    /// `hover` over the window colour, opaque: a pointed-at tab row, and its close button.
    pub row_hover: Color,
    /// The current space's colour.
    pub accent: Color,
    /// The other spaces' dots.
    pub dot: Color,
    /// The address field in the top toolbar, on the page panel.
    pub toolbar_field: Color,
    pub toolbar_border: Color,
}

/// Each space's colour, light then dark: evergreen, cobalt, rust, plum.
const ACCENTS: [(u32, u32); 4] = [(0x2E5E4E, 0x5FA58A), (0x3460C8, 0x7EA6F0), (0xB0602A, 0xE0955A), (0x80489A, 0xB98ACB)];

pub fn accent(space: usize, dark: bool) -> Color {
    let (light, night) = ACCENTS[space % ACCENTS.len()];
    rgb(if dark { night } else { light })
}

/// The page panel's colour as 0xAARRGGBB, for Chromium.
pub fn page_argb(dark: bool) -> u32 {
    if dark { 0xFF18_1B1D } else { 0xFFFF_FFFF }
}

pub fn palette(dark: bool, space: usize) -> Palette {
    if dark {
        Palette {
            dark,
            window: rgb(0x121517),
            field: rgb(0x1E2326),
            page: rgb(0x181B1D),
            tab_active: rgb(0x262C2F),
            tab_shadow: rgba(0x000000, 0.35),
            tab_ring: rgba(0xFFFFFF, 0.06),
            page_ring: rgba(0xFFFFFF, 0.07),
            page_shadow: rgba(0x000000, 0.30),
            text: rgb(0xE8ECEE),
            text_strong: rgb(0xF3F5F6),
            text_soft: rgb(0xD3D9DC),
            text_strip: rgb(0xBCC4C8),
            muted: rgb(0x8E979C),
            faint: rgb(0x7F888D),
            icon: rgb(0x9AA3A8),
            divider: rgb(0x262C2F),
            hover: rgba(0xFFFFFF, 0.06),
            row_hover: rgb(0x202325),
            accent: accent(space, true),
            dot: rgb(0x394145),
            toolbar_field: rgb(0x22272A),
            toolbar_border: rgb(0x22272A),
        }
    } else {
        Palette {
            dark,
            window: rgb(0xEEF1F2),
            field: rgb(0xE1E6E8),
            page: rgb(0xFFFFFF),
            tab_active: rgb(0xFFFFFF),
            tab_shadow: rgba(0x1C2124, 0.07),
            tab_ring: rgba(0x1C2124, 0.06),
            page_ring: rgba(0x1C2124, 0.08),
            page_shadow: rgba(0x1C2124, 0.06),
            text: rgb(0x1C2124),
            text_strong: rgb(0x1C2124),
            text_soft: rgb(0x1C2124),
            text_strip: rgb(0x4A5257),
            muted: rgb(0x6B7378),
            faint: rgb(0x8A9196),
            icon: rgb(0x6B7378),
            divider: rgb(0xDDE2E4),
            hover: rgba(0x1C2124, 0.05),
            row_hover: rgb(0xE3E7E8),
            accent: accent(space, false),
            dot: rgb(0xC3CACD),
            toolbar_field: rgb(0xF1F3F4),
            toolbar_border: rgb(0xEEF1F2),
        }
    }
}
