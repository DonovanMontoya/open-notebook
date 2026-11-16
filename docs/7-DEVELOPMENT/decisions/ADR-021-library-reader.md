# ADR-021: Library opens materials independently of notebooks

- **Status**: Accepted
- **Date**: 2026-11-15

## Context

Library previously opened a source in a notebook, beside chat and with generated insights inside the reader. Reading therefore required a notebook and competed with research controls.

## Decision

The Rune Library owns its source-loading and reader state. Opening a material stays in Library and shows retained original PDF pages on Mac, or formatted extracted text in a bounded reading column with text sizing and literal find. Sources without notebook membership can also be read. A separate Manage materials view retains notebook membership controls. The notebook workspace owns generated findings and processing tools; PDF questions sit beside the material in a dedicated Discussion panel.

## Consequences

Library does not load generated findings to display a material. Original web links remain available; file downloads and processing tools remain in the notebook workspace. Reading preferences and position are session state, not durable bookmarks.

## Original PDFs on macOS

Retained original PDFs render in a PDFKit view embedded in Library's content region, inside the existing app window. Extracted text remains the research representation and is not used to reconstruct PDF pages. Rune owns the header and layout rectangle; the native view follows that rectangle, exposes its controls in the window's accessibility tree, and is removed when Library is left or a modal opens.

The Swift renderer is compiled as a dynamic library for the target Mac architecture and embedded in the Rust binary. It loads in the app process and consumes original PDF bytes directly, without a second app, window, or document copy on disk. The module remains loaded for the process lifetime so native callbacks cannot outlive their code. iOS retains the text reader until a native PDF surface is implemented there.

## Questions about passages and figures

PDFKit selection events cross a polled in-process event bridge into Library's question composer. A local mouse-up monitor captures selections after PDFKit's internal document view handles them; overriding PDFView's mouse-up does not receive those events. Highlight annotations affect the in-memory document only.

Numbered captions identify figures. Each figure carries its caption, PDF page number, and passages mentioning that figure number. Links navigate to and select the related text. Figure questions attach a rendered original page to the current user turn through the existing source-chat backend. Sending the full page preserves diagrams and multi-panel charts without relying on uncertain crop boundaries. OpenAI-compatible, Anthropic and Gemini serializers carry image content; the server source-chat endpoint rejects images explicitly instead of silently dropping them.

Conversation text uses existing source-chat persistence and notebook membership. Images and the pinned context are session state; a selected figure is reattached to each follow-up while pinned. Scanned pages and unlabeled figures require future OCR or visual segmentation. Model setup remains explicit in Settings.
