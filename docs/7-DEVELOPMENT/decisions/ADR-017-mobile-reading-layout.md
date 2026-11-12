# ADR-017: Mobile reading layout

Audit: a 256px desktop sidebar leaves only 134px of a 390px viewport. Notebook toolbars, model selectors and episode actions then exceed their columns and are clipped. Some fields and labels are too small for touch; page padding compounds the narrow space.

Direction: Open Notebook is a personal reading and study workspace. Keep its calm, precise, slightly warm character, Instrument Sans interface, Bricolage headings, neutral reading surfaces, fern actions, and three-color mark. The content and playback controls carry emphasis. Preserve desktop layouts and use a Radix dialog drawer below 768px, full-width content and 16px phone page gutters. Keep words together and break only unbroken strings. Let action rows stack, selectors shrink, and dialogs scroll within the dynamic viewport. Retain the existing notebook underline as the signature. No new decorative images or motion.

Applied frontend-design guidance from the Razer Blade installation and the Unslop polish workflow by Damir Sokolovsky (@heydamir), https://heydamir.design/unslop, CC BY 4.0. This is a light-slop correction of layout and craft, preserving the established brand. The user explicitly requested the implementation, so the existing authorization covers the polish direction.

GPT Live remains a conversational session model, distinct from saved-podcast speech synthesis. The speaker form now explains that limitation rather than advertising an incompatible speech endpoint.


Validation: production build, frontend lint (existing warnings only), and 189 frontend tests pass. Chromium touch layouts at 320/390px and tablet/desktop at 768/1440px were checked across notebooks, podcasts, models, usage and notebook chat. No clipped out-of-viewport content remained outside intentional scroll regions. Drawer navigation/Escape dismissal and persistent podcast audio passed at both phone widths. WebKit could not launch on this host, so actual Safari verification remains outstanding. Existing palette values retained: reading surface #f5f5f2, card #fefefc, ink #23252a, secondary ink #565a61, fern #245740, teal #0e7268.
