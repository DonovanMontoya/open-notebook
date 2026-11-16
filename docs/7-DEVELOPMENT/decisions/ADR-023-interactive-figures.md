# ADR-023: Interactive figure explanations in Library

- **Status**: Accepted
- **Date**: 2026-11-15

## Context

Original PDF pages and a discussion panel support reading and questions, but static text alone is insufficient for explaining diagrams, axes and relationships. The discussion also needs a width appropriate to the reader's task.

## Decision

The Library split has a draggable, accessible separator with bounded widths. The preferred discussion width is stored in app preferences. A smaller window clamps the visible width while retaining the preference for when space returns. Double-click restores the default.

Figure questions can request a self-contained HTML document. A complete fenced HTML block becomes an interactive-document action in the answer. The original answer, including its HTML, uses existing source-chat persistence. A completed new visualization opens beside the discussion in the Library content region. The user can return to the original paper or restart the document; page citations return to the PDF.

The native PDF host owns a lazy WebKit surface using a nonpersistent data store. Generated content runs in a sandboxed, opaque-origin iframe with scripts allowed. Host HTML encodes the generated document as a JSON string with markup delimiters escaped. Content security policy disallows external resources and connections; navigation policy rejects non-about navigation. No credentials, file bridge, downloads or arbitrary host commands are exposed. A message is accepted only from the expected frame, and the native receiver accepts only main-frame forwarding of a bounded PDF page number or a short runtime error.

The host executes only a completed HTML block under 512 KiB. Streaming code is hidden behind a building message. Leaving visualization mode unloads the frame and stops its scripts. The generation instructions require animation controls, paused initial state, reduced-motion behavior, readable labels and a clear distinction between source measurements and schematic interpretation. These instructions guide generated content; the host does not claim to validate scientific interpretations.

## Consequences

The reader stays in Library and the conversation stays visible while an explanation is explored. HTML can use inline SVG, CSS and JavaScript, including animations and interactive controls. External libraries and resources are unavailable, and incomplete or oversized output requires regeneration. The original PDF remains the authoritative material. Scanned and unlabeled-figure detection remains outside this change.
