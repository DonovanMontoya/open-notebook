//! While a reply streams in, show only the Markdown blocks that are finished:
//! everything up to the last blank line that is not inside a code fence. The
//! block still being written stays hidden, so paragraphs, lists and tables
//! appear whole instead of word by word. Port of `packages/engine/src/blocks.ts`.

pub fn finished_blocks(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut in_fence = false;
    let mut last_boundary = 0;
    let mut line_start = 0;
    for i in 0..=bytes.len() {
        if i < bytes.len() && bytes[i] != b'\n' {
            continue;
        }
        let line = &text[line_start..i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            // A closing fence ends a block on its own.
            if !in_fence && i < bytes.len() {
                last_boundary = i + 1;
            }
        } else if !in_fence && line.trim().is_empty() && i < bytes.len() && line_start > 0 {
            last_boundary = line_start;
        }
        line_start = i + 1;
    }
    text[..last_boundary].trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_finished_paragraphs_are_shown_while_streaming() {
        assert_eq!(finished_blocks("First para"), "");
        assert_eq!(finished_blocks("First para\n\nSecond is still bei"), "First para");
        assert_eq!(finished_blocks("First para\n\n"), "First para");
        assert_eq!(finished_blocks("One\n\nTwo\n\n- a\n- b"), "One\n\nTwo");
    }

    #[test]
    fn blank_lines_inside_an_open_code_fence_do_not_end_a_block() {
        assert_eq!(finished_blocks("Intro\n\n```py\nx = 1\n\ny = 2"), "Intro");
        let closed = "Intro\n\n```py\nx = 1\n\ny = 2\n```\nAfter the fence";
        assert_eq!(finished_blocks(closed), "Intro\n\n```py\nx = 1\n\ny = 2\n```");
    }
}
