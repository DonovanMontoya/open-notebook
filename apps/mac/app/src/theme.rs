//! A reading desk under a lamp: graphite surfaces, Newsreader for everything you read, and one
//! accent, library amber, kept for citations, the selection and the passage a citation points at.
//! The same tokens as `apps/desktop/src/styles.css`.

use rune::tok::{Family, TextStyle};
use rune::{Color, color::hex};
use rune_kit::{Kit, PaletteSeed};

pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/Inter-Regular.ttf"),
    include_bytes!("../assets/Inter-Medium.ttf"),
    include_bytes!("../assets/Inter-SemiBold.ttf"),
    include_bytes!("../assets/Newsreader.ttf"),
];
pub const FAMILIES: (&str, &str) = ("Inter", "ui-monospace");

const READ: Family = Family::Named("Newsreader");

/// Reading text: chat answers, notes, source text.
pub const READ_BODY: TextStyle = TextStyle::new(16.0, 400.0).family(READ).lh(25.0);
pub const READ_TITLE: TextStyle = TextStyle::new(26.0, 500.0).family(READ).lh(32.0);
pub const READ_HEADING: TextStyle = TextStyle::new(19.0, 500.0).family(READ).lh(26.0);

/// This device, as copy names it.
#[cfg(target_os = "ios")]
pub const THIS_DEVICE: &str = "this iPhone";
#[cfg(not(target_os = "ios"))]
pub const THIS_DEVICE: &str = "this Mac";

/// A style with tabular figures, for counts, times and prices that line up.
pub const fn tnum(style: TextStyle) -> TextStyle {
    style.features("'tnum' 1")
}

/// How much room the window has, which decides the layout. Measured from the window, never
/// from the device: a folded iPhone Duo, an open one and a Mac window can each be any of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// One column, with a back bar and tabs at the bottom: an iPhone, or a folded Duo.
    Phone,
    /// Two facing pages: an open iPhone Duo, an iPad, a narrow Mac window.
    Pages,
    /// Sidebar, sources and the notebook side by side: the Mac.
    Desk,
}

impl Size {
    pub fn of(width: f32) -> Size {
        if width < 560.0 {
            Size::Phone
        } else if width < 900.0 {
            Size::Pages
        } else {
            Size::Desk
        }
    }
}

#[derive(Clone)]
pub struct Look {
    pub k: Kit,
    pub dark: bool,
    pub size: Size,
    /// The window's width in points.
    pub width: f32,
    /// Room to leave under the last row for the home indicator.
    pub inset_bottom: f32,
    pub page: Color,
    pub panel: Color,
    pub raised: Color,
    pub text: Color,
    pub text2: Color,
    pub text3: Color,
    pub line: Color,
    pub line_strong: Color,
    pub amber: Color,
    pub amber_wash: Color,
    pub danger: Color,
    pub ok: Color,
}

fn c(s: &str) -> Color {
    hex(s).expect("valid colour")
}

impl Look {
    pub fn new(dark: bool) -> Look {
        let (page, panel, raised, text, text2, text3) = if dark {
            ("#141415", "#19191b", "#232325", "#ededed", "#a1a1a6", "#85858b")
        } else {
            ("#fafafa", "#ffffff", "#f4f4f5", "#171717", "#525252", "#737373")
        };
        let amber = if dark { c("#f0b25a") } else { c("#a8650f") };
        let ink = c(text);
        let mut k = Kit::from_palette(PaletteSeed { bg: c(page), fg: ink, accent: amber });
        // Primary buttons are ink on paper, as in the web app; amber stays for citations.
        k.c.text_base = ink;
        k.c.text_muted = c(text2);
        k.c.text_faint = c(text3);
        Look {
            dark,
            size: Size::Desk,
            width: 1280.0,
            inset_bottom: 0.0,
            page: c(page),
            panel: c(panel),
            raised: c(raised),
            text: ink,
            text2: c(text2),
            text3: c(text3),
            line: ink.alpha(0.08),
            line_strong: ink.alpha(0.14),
            amber,
            amber_wash: c("#f0b25a").alpha(if dark { 0.16 } else { 0.24 }),
            danger: if dark { c("#e0625c") } else { c("#b4403a") },
            ok: if dark { c("#6fb48a") } else { c("#3f7f57") },
            k,
        }
    }

    /// The same look for a window `width` points wide.
    pub fn sized(mut self, width: f32) -> Look {
        self.size = Size::of(width);
        self.width = width;
        self
    }

    pub fn phone(&self) -> bool {
        self.size == Size::Phone
    }

    /// Side padding for a page: tighter on a phone.
    pub fn gutter(&self) -> f32 {
        if self.phone() { 16.0 } else { 32.0 }
    }

    /// Side padding inside a page's column.
    pub fn pad(&self) -> f32 {
        if self.phone() { 16.0 } else { 24.0 }
    }

    /// Space above a page's title.
    pub fn pad_top(&self) -> f32 {
        if self.phone() { 12.0 } else { 28.0 }
    }

    /// A dialog's width: `want`, or the window less a 16 point margin each side.
    pub fn dialog(&self, want: f32) -> f32 {
        want.min(self.width - 32.0).max(240.0)
    }
}
