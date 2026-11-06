//! Just enough of a DOM over quick-xml for the Office readers: elements by
//! local name (namespace prefixes ignored), attributes and text content.

use quick_xml::events::Event;
use quick_xml::reader::Reader;

#[derive(Debug, Default)]
pub struct Element {
    /// The name without its namespace prefix, e.g. "t" for `<w:t>`.
    pub local: String,
    attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug)]
pub enum Node {
    Element(Element),
    Text(String),
}

/// Parses a document leniently; it returns what it read so far on malformed XML.
pub fn parse(source: &str) -> Element {
    let mut reader = Reader::from_str(source);
    reader.config_mut().check_end_names = false;
    let mut stack: Vec<Element> = vec![Element::default()];
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => {
                let el = element(&e);
                push(&mut stack, Node::Element(el));
            }
            Event::End(_) if stack.len() > 1 => {
                let done = stack.pop().unwrap_or_default();
                push(&mut stack, Node::Element(done));
            }
            Event::Text(t) => push_text(&mut stack, &t.xml10_content()),
            Event::CData(c) => push_text(&mut stack, &c),
            Event::GeneralRef(r) => {
                let name = r.to_string();
                let text = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => quick_xml::escape::resolve_xml_entity(&name).map(str::to_string).unwrap_or_default(),
                };
                push_text(&mut stack, &text);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    while stack.len() > 1 {
        let done = stack.pop().unwrap_or_default();
        push(&mut stack, Node::Element(done));
    }
    stack.pop().unwrap_or_default()
}

fn element(e: &quick_xml::events::BytesStart<'_>) -> Element {
    let local = e.local_name().as_ref().to_string();
    let attrs = e
        .attributes()
        .flatten()
        .map(|a| {
            let key = a.key.local_name().as_ref().to_string();
            let value = a.normalized_value(quick_xml::XmlVersion::default()).map(|v| v.into_owned()).unwrap_or_else(|_| a.value.to_string());
            (key, value)
        })
        .collect();
    Element { local, attrs, children: Vec::new() }
}

fn push(stack: &mut [Element], node: Node) {
    if let Some(top) = stack.last_mut() {
        top.children.push(node);
    }
}

fn push_text(stack: &mut [Element], text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(top) = stack.last_mut() {
        if let Some(Node::Text(prev)) = top.children.last_mut() {
            prev.push_str(text);
        } else {
            top.children.push(Node::Text(text.to_string()));
        }
    }
}

impl Element {
    /// An attribute by local name.
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == local).map(|(_, v)| v.as_str())
    }

    pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// Every descendant element in document order, not including self.
    pub fn descendants(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        fn walk<'a>(e: &'a Element, out: &mut Vec<&'a Element>) {
            for c in e.child_elements() {
                out.push(c);
                walk(c, out);
            }
        }
        walk(self, &mut out);
        out
    }

    /// Descendants with this local name, like `getElementsByTagName('*')` filtered by localName.
    pub fn all(&self, local: &str) -> Vec<&Element> {
        self.descendants().into_iter().filter(|e| e.local == local).collect()
    }

    pub fn first(&self, local: &str) -> Option<&Element> {
        self.all(local).into_iter().next()
    }

    /// `textContent`.
    pub fn text(&self) -> String {
        let mut out = String::new();
        fn walk(e: &Element, out: &mut String) {
            for c in &e.children {
                match c {
                    Node::Text(t) => out.push_str(t),
                    Node::Element(e) => walk(e, out),
                }
            }
        }
        walk(self, &mut out);
        out
    }

    /// The text of every `<t>` under this element, joined.
    pub fn texts(&self, sep: &str) -> String {
        self.all("t").iter().map(|t| t.text()).collect::<Vec<_>>().join(sep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_local_names_attributes_and_entities() {
        let doc = parse(r#"<?xml version="1.0"?><w:doc xmlns:w="x"><w:p w:id="1"><w:t>a &amp; b</w:t><w:br/><w:t>&#233;</w:t></w:p></w:doc>"#);
        let p = doc.first("p").unwrap();
        assert_eq!(p.attr("id"), Some("1"));
        assert_eq!(p.texts("|"), "a & b|é");
        assert_eq!(p.child_elements().map(|e| e.local.as_str()).collect::<Vec<_>>(), ["t", "br", "t"]);
    }

    #[test]
    fn survives_unclosed_tags() {
        let doc = parse("<a><b>text");
        assert_eq!(doc.text(), "text");
    }
}
