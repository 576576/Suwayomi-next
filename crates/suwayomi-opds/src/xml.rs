//! Minimal XML writer with escaping — mirrors `opds/util/OpdsXmlUtil.kt`
//! (hand-rolled, no external XML dependency).

/// XML-escapes text content / attribute values.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Simple push-based XML writer (namespace-prefixed elements).
#[derive(Default)]
pub struct XmlWriter {
    buf: String,
}

impl XmlWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn declaration(&mut self) {
        self.buf.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    }

    /// `<name attr="v" ...>children</name>` — no children yields
    /// `<name ...></name>` (explicit close; harmless for OPDS readers).
    pub fn element(&mut self, name: &str, attrs: &[(&str, &str)], children: impl FnOnce(&mut Self)) {
        self.buf.push('<');
        self.buf.push_str(name);
        for (k, v) in attrs {
            self.buf.push(' ');
            self.buf.push_str(k);
            self.buf.push_str("=\"");
            self.buf.push_str(&escape(v));
            self.buf.push('"');
        }
        self.buf.push('>');
        children(self);
        self.buf.push_str("</");
        self.buf.push_str(name);
        self.buf.push('>');
    }

    /// Self-closing `<name .../>`.
    pub fn void(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.buf.push('<');
        self.buf.push_str(name);
        for (k, v) in attrs {
            self.buf.push(' ');
            self.buf.push_str(k);
            self.buf.push_str("=\"");
            self.buf.push_str(&escape(v));
            self.buf.push('"');
        }
        self.buf.push_str("/>");
    }

    /// `<name>escaped-text</name>`.
    pub fn leaf(&mut self, name: &str, text: &str) {
        self.buf.push('<');
        self.buf.push_str(name);
        self.buf.push('>');
        self.buf.push_str(&escape(text));
        self.buf.push_str("</");
        self.buf.push_str(name);
        self.buf.push('>');
    }

    /// Appends escaped text (for use inside an open element).
    pub fn raw_text(&mut self, text: &str) {
        self.buf.push_str(&escape(text));
    }

    pub fn finish(self) -> String {
        self.buf
    }
}
