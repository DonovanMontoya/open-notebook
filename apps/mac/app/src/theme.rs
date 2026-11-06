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

#[derive(Clone)]
pub struct Look {
    pub k: Kit,
    pub dark: bool,
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
            danger: if dark { c("#e5484d") } else { c("#c8322f") },
            k,
        }
    }
}
