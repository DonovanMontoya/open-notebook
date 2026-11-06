//! Ask: research across the whole knowledge base, following the server's
//! three-step method (prompts/ask): plan searches, answer each from what it
//! finds, then write one final answer that cites documents by id.
//! Port of `packages/engine/src/ask.ts`.

use crate::text::json_object;
use crate::types::AskSearch;

const ROLE: &str = "You are a research assistant who helps users learn from documents in their workspace.";
pub const MAX_SEARCHES: usize = 4;

pub fn strategy_prompt(question: &str, max_searches: usize) -> String {
    [
        ROLE.to_string(),
        "First step: plan how to find the information that answers the question.".into(),
        format!("Explain your reasoning briefly, then give up to {max_searches} search queries. Each has a search term and instructions for what to extract from the results."),
        "Answer with JSON only, in this shape:".into(),
        r#"{"reasoning": "...", "searches": [{"term": "...", "instructions": "..."}]}"#.into(),
        format!("# Question\n{question}"),
    ]
    .join("\n\n")
}

/// The plan in a model reply; no searches when it can't be read.
pub fn parse_strategy(raw: &str, max_searches: usize) -> (String, Vec<AskSearch>) {
    if let Ok(data) = json_object(raw) {
        let field = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.trim().to_string()).unwrap_or_default();
        let searches: Vec<AskSearch> = data
            .get("searches")
            .and_then(|s| s.as_array())
            .map(|list| {
                list.iter()
                    .map(|s| AskSearch { term: field(s, "term"), instructions: field(s, "instructions") })
                    .filter(|s| !s.term.is_empty())
                    .take(max_searches)
                    .collect()
            })
            .unwrap_or_default();
        if !searches.is_empty() {
            let reasoning = data.get("reasoning").and_then(|r| r.as_str()).unwrap_or("").to_string();
            return (reasoning, searches);
        }
    }
    (String::new(), Vec::new())
}

pub struct AskDoc<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub text: &'a str,
}

pub fn search_answer_prompt(question: &str, search: &AskSearch, results: &[AskDoc<'_>]) -> String {
    let mut ids: Vec<&str> = Vec::new();
    for r in results {
        if !ids.contains(&r.id) {
            ids.push(r.id);
        }
    }
    let instructions = if search.instructions.is_empty() { "Summarize what the results say about the term." } else { &search.instructions };
    let mut parts = vec![
        ROLE.to_string(),
        format!("# Question\n{question}"),
        format!("# Search\nTerm: {}\nInstructions: {instructions}", search.term),
        "# Results".into(),
    ];
    for r in results {
        parts.push(format!("<document id=\"{}\" title=\"{}\">\n{}\n</document>", r.id, r.title.replace('"', "'"), r.text));
    }
    parts.push(
        [
            "# Your job".to_string(),
            "Answer from these results only. Cite each claim with the document id in square brackets, like [source:abc].".into(),
            "Use ids exactly as given; never invent ids. If the results do not cover it, say so.".into(),
            format!("Ids you may cite: {}", if ids.is_empty() { "none".to_string() } else { ids.join(", ") }),
        ]
        .join("\n"),
    );
    parts.join("\n\n")
}

pub fn final_answer_prompt(question: &str, reasoning: &str, answers: &[(String, String)]) -> String {
    let mut parts = vec![
        ROLE.to_string(),
        "Last step: write the final answer to the question from the partial answers below.".into(),
        format!("# Question\n{question}"),
        if reasoning.is_empty() { String::new() } else { format!("# Your search strategy\n{reasoning}") },
        "# Partial answers".into(),
    ];
    for (term, answer) in answers {
        parts.push(format!("## {term}\n{answer}"));
    }
    parts.push(
        [
            "# Your job",
            "Write a clear, complete answer in Markdown. Keep the citations from the partial answers, using the same [id] form.",
            "Only cite ids that appear in the partial answers. If the documents do not answer the question, say what is missing.",
        ]
        .join("\n"),
    );
    parts.retain(|p| !p.is_empty());
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strategy_reads_the_plan_and_caps_the_searches() {
        let (reasoning, searches) = parse_strategy(
            "```json\n{\"reasoning\":\"r\",\"searches\":[{\"term\":\"a\",\"instructions\":\"x\"},{\"term\":\"\"},{\"term\":\"b\"},{\"term\":\"c\"},{\"term\":\"d\"},{\"term\":\"e\"}]}\n```",
            MAX_SEARCHES,
        );
        assert_eq!(reasoning, "r");
        assert_eq!(searches.iter().map(|s| s.term.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d"]);
        assert!(parse_strategy("not json", MAX_SEARCHES).1.is_empty());
    }
}
