//! The notebook chat system prompt and how a context budget is shared.
//! Port of `packages/engine/src/chat.ts`.

#[derive(Clone, Debug, PartialEq)]
pub struct ContextSource {
    pub title: String,
    pub text: String,
}

/// An excerpt of a source too long to send whole.
pub type ContextChunk = ContextSource;

#[derive(Clone, Debug, PartialEq)]
pub struct ContextNote {
    pub title: String,
    pub content: String,
}

const NOTES_BUDGET: usize = 40_000;

/// System prompt for notebook chat: instructions first, then the notebook material.
pub fn build_chat_prompt(
    name: &str,
    description: &str,
    sources: &[ContextSource],
    chunks: &[ContextChunk],
    notes: &[ContextNote],
) -> String {
    let mut parts: Vec<String> = vec![
        format!("You are a research assistant working inside the notebook \"{name}\"."),
        if description.is_empty() { String::new() } else { format!("Notebook description: {description}") },
        [
            "Answer from the notebook material below. When you use it, cite the source title in square brackets, like [Title].",
            "If the material does not cover the question, say so plainly, then answer from general knowledge and mark that part as such.",
            "Use Markdown for structure when it helps.",
        ]
        .join(" "),
    ];

    if !sources.is_empty() {
        parts.push("# Sources".into());
        for s in sources {
            parts.push(format!("<source title=\"{}\">\n{}\n</source>", escape_attr(&s.title), s.text.trim()));
        }
    }
    if !chunks.is_empty() {
        parts.push(if sources.is_empty() {
            "# Source excerpts (each source is represented by its opening and the passages most relevant to the question)".into()
        } else {
            "# Excerpts from longer sources (each is too long to include whole, so it is represented by its opening and the passages most relevant to the question)".into()
        });
        for (title, texts) in group_by_title(chunks) {
            let body = texts.iter().map(|t| t.trim()).collect::<Vec<_>>().join("\n[…]\n");
            parts.push(format!("<source title=\"{}\" excerpts=\"true\">\n{body}\n</source>", escape_attr(title)));
        }
    }
    if sources.is_empty() && chunks.is_empty() {
        parts.push("This notebook has no processed sources yet.".into());
    }

    let notes: Vec<&ContextNote> = notes.iter().filter(|n| !n.content.trim().is_empty()).collect();
    if !notes.is_empty() {
        parts.push("# Notes the user wrote or saved".into());
        let mut used = 0;
        for n in notes {
            let body = n.content.trim();
            let len = body.chars().count();
            if used + len > NOTES_BUDGET {
                break;
            }
            used += len;
            parts.push(format!("<note title=\"{}\">\n{body}\n</note>", escape_attr(&n.title)));
        }
    }

    parts.retain(|p| !p.is_empty());
    parts.join("\n\n")
}

fn escape_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_break = false;
    for c in value.chars() {
        if c == '\r' || c == '\n' {
            if !in_break {
                out.push(' ');
            }
            in_break = true;
            continue;
        }
        in_break = false;
        out.push(if c == '"' { '\'' } else { c });
    }
    out
}

fn group_by_title(chunks: &[ContextChunk]) -> Vec<(&str, Vec<&str>)> {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for c in chunks {
        match groups.iter_mut().find(|(t, _)| *t == c.title) {
            Some((_, texts)) => texts.push(&c.text),
            None => groups.push((&c.title, vec![&c.text])),
        }
    }
    groups
}

/// Splits a character budget across sources so each gets an equal share, and short sources hand
/// what they don't use to the longer ones. Returns each source's allowance, in input order.
pub fn fair_share(lengths: &[usize], budget: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..lengths.len()).collect();
    order.sort_by_key(|&i| lengths[i]);
    let mut out = vec![0; lengths.len()];
    let mut left = budget;
    for (k, &i) in order.iter().enumerate() {
        out[i] = lengths[i].min(left / (order.len() - k));
        left -= out[i];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(title: &str, text: &str) -> ContextSource {
        ContextSource { title: title.into(), text: text.into() }
    }

    #[test]
    fn chat_prompt_cites_sources_by_title_and_notes_when_nothing_is_processed() {
        let prompt = build_chat_prompt("N \"1\"", "", &[], &[], &[ContextNote { title: "Line\nbreak".into(), content: "body".into() }]);
        assert!(prompt.contains("notebook \"N \"1\"\""));
        assert!(prompt.contains("no processed sources yet"));
        assert!(prompt.contains("<note title=\"Line break\">"));
    }

    #[test]
    fn fair_share_gives_short_sources_what_they_need_and_splits_the_rest_evenly() {
        assert_eq!(fair_share(&[10, 1000, 1000], 610), [10, 300, 300]);
        assert_eq!(fair_share(&[5, 5], 100), [5, 5]);
        assert!(fair_share(&[], 100).is_empty());
    }

    #[test]
    fn chat_prompt_keeps_whole_sources_and_excerpts_together() {
        let prompt = build_chat_prompt("N", "", &[src("Short (insights)", "summary")], &[src("Long", "opening"), src("Long", "later")], &[]);
        assert!(prompt.contains("<source title=\"Short (insights)\">"));
        assert!(prompt.contains("<source title=\"Long\" excerpts=\"true\">\nopening\n[…]\nlater\n</source>"));
    }
}
