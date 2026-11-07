//! Line icons on a 16-point grid, drawn at 1.2 points like the kit's own.

use rune::Icon;

macro_rules! icon {
    ($id:ident, $name:literal, $body:literal) => {
        pub static $id: Icon = Icon { name: $name, view: 16.0, body: $body };
    };
}

icon!(PLUS, "on-plus", r#"<path d="M8 3.5V12.5M3.5 8H12.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#);
icon!(
    BOOK,
    "on-book",
    r#"<path d="M3.5 3.5C3.5 2.95 3.95 2.5 4.5 2.5H12.5V11.5H4.5C3.95 11.5 3.5 11.95 3.5 12.5V3.5Z M3.5 12.5C3.5 13.05 3.95 13.5 4.5 13.5H12.5V11.5" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#
);
icon!(
    GEAR,
    "on-gear",
    r#"<circle cx="8" cy="8" r="2" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M8 2V3.5M8 12.5V14M2 8H3.5M12.5 8H14M3.75 3.75L4.8 4.8M11.2 11.2L12.25 12.25M3.75 12.25L4.8 11.2M11.2 4.8L12.25 3.75" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#
);
icon!(
    TRASH,
    "on-trash",
    r#"<path d="M3 4.5H13M6.5 4.5V3H9.5V4.5M4.5 4.5L5 13H11L11.5 4.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
icon!(
    SEND,
    "on-send",
    r#"<path d="M8 12.5V3.5M4 7.5L8 3.5L12 7.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
icon!(STOP, "on-stop", r#"<rect x="4.5" y="4.5" width="7" height="7" rx="1.2" fill="currentColor"/>"#);
icon!(
    LINK,
    "on-link",
    r#"<path d="M7 9L9 7M6 5.5L7.2 4.3C8.3 3.2 10 3.2 11.1 4.3V4.3C12.2 5.4 12.2 7.1 11.1 8.2L10 9.5M9.5 10.5L8.3 11.7C7.2 12.8 5.5 12.8 4.4 11.7V11.7C3.3 10.6 3.3 8.9 4.4 7.8L5.5 6.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"/>"#
);
icon!(
    DOC,
    "on-doc",
    r#"<path d="M4 2.5H9.5L12 5V13.5H4V2.5Z M9.5 2.5V5H12 M6 8H10M6 10.5H10" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#
);
icon!(CLOSE, "on-close", r#"<path d="M4.5 4.5L11.5 11.5M11.5 4.5L4.5 11.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#);
icon!(
    RETRY,
    "on-retry",
    r#"<path d="M12.5 8A4.5 4.5 0 1 1 11.2 4.8M12.5 3V5.5H10" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
icon!(SAVE, "on-save", r#"<path d="M4.5 2.5H11.5V13.5L8 11L4.5 13.5V2.5Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#);
icon!(
    GAUGE,
    "on-gauge",
    r#"<path d="M2.5 11.5A5.5 5.5 0 1 1 13.5 11.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"/><path d="M8 11L10.5 6.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#
);
icon!(
    CHAT,
    "on-chat",
    r#"<path d="M3 4.5C3 3.67 3.67 3 4.5 3H11.5C12.33 3 13 3.67 13 4.5V9.5C13 10.33 12.33 11 11.5 11H7L4.5 13V11C3.67 11 3 10.33 3 9.5V4.5Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#
);
icon!(
    SERVER,
    "on-server",
    r#"<rect x="3" y="3" width="10" height="4" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/><rect x="3" y="9" width="10" height="4" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/><circle cx="5.5" cy="5" r="0.7" fill="currentColor"/><circle cx="5.5" cy="11" r="0.7" fill="currentColor"/>"#
);
icon!(PLAY, "on-play", r#"<path d="M5.5 3.8V12.2L12 8L5.5 3.8Z" fill="currentColor"/>"#);
icon!(
    PAUSE,
    "on-pause",
    r#"<rect x="4.5" y="3.5" width="2.5" height="9" rx="0.8" fill="currentColor"/><rect x="9" y="3.5" width="2.5" height="9" rx="0.8" fill="currentColor"/>"#
);
icon!(
    DEVICE,
    "on-device",
    r#"<rect x="2.5" y="3.5" width="11" height="7.5" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M1.5 13H14.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#
);
icon!(TEXT, "on-text", r#"<path d="M3 4H13M3 6.67H13M3 9.33H13M3 12H9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#);
icon!(
    EXTERNAL,
    "on-external",
    r#"<path d="M9 3H13V7M13 3L7.5 8.5M11 9.5V12.5C11 12.78 10.78 13 10.5 13H3.5C3.22 13 3 12.78 3 12.5V5.5C3 5.22 3.22 5 3.5 5H6.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
icon!(
    DOWNLOAD,
    "on-download",
    r#"<path d="M8 2.5V10M4.5 6.5L8 10L11.5 6.5M3 13H13" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
);
