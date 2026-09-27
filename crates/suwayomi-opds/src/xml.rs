//! Minimal XML writer with escaping — mirrors `opds/util/OpdsXmlUtil.kt`
//! (hand-rolled, no external XML dependency).
//!
//! 结构是**不可变树**：先构造 [`XmlNode`]，再由纯函数
//! [`XmlNode::render`] 折叠成字符串。旧实现是一个 `&mut self` 的 push 构建器
//! ——节点写出即不可见；树可以复用、可以断言、可以差分，`render` 没有副作用。

use std::fmt::Write as _;

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

/// 属性表。名字都是编译期字面量，所以用 `&'static str` 而不是 `String`。
pub type Attrs = Vec<(&'static str, String)>;

/// 一个 XML 节点。
///
/// 文本与属性值都保存**未转义**的原文，转义留给渲染阶段——这样两棵树可以
/// 直接比较，不必先各自渲染成字符串。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlNode {
    /// `<name attr="v">children</name>`
    Element { name: &'static str, attrs: Attrs, children: Vec<Self> },
    /// `<name attr="v"/>`
    Void { name: &'static str, attrs: Attrs },
    /// 元素内的文本（渲染时转义）
    Text(String),
}

impl XmlNode {
    /// `<name attr="v" ...>children</name>` —— 没有子节点时输出
    /// `<name ...></name>`（显式闭合；OPDS 阅读器对此无异议）。
    pub fn element(name: &'static str, attrs: Attrs, children: Vec<Self>) -> Self {
        Self::Element { name, attrs, children }
    }

    /// 自闭合 `<name attr="v" .../>`。
    pub fn void(name: &'static str, attrs: Attrs) -> Self {
        Self::Void { name, attrs }
    }

    /// `<name>escaped-text</name>`。
    pub fn leaf(name: &'static str, text: &str) -> Self {
        Self::element(name, Vec::new(), vec![Self::Text(text.to_string())])
    }

    /// 元素内的裸文本（渲染时转义）。
    pub fn text(content: &str) -> Self {
        Self::Text(content.to_string())
    }

    /// 纯函数：节点 → 字符串（不含 XML 声明）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out);
        out
    }

    /// 纯函数：节点 → 带 XML 声明的完整文档。
    pub fn render_document(&self) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        self.write_into(&mut out);
        out
    }

    /// 把节点折叠进已有缓冲。只在这里碰可变状态，且作用域仅限于一次遍历。
    fn write_into(&self, out: &mut String) {
        match self {
            Self::Text(content) => out.push_str(&escape(content)),
            Self::Void { name, attrs } => {
                let _ = write!(out, "<{name}");
                write_attrs(attrs, out);
                out.push_str("/>");
            }
            Self::Element { name, attrs, children } => {
                let _ = write!(out, "<{name}");
                write_attrs(attrs, out);
                out.push('>');
                for child in children {
                    child.write_into(out);
                }
                let _ = write!(out, "</{name}>");
            }
        }
    }
}

fn write_attrs(attrs: &Attrs, out: &mut String) {
    for (key, value) in attrs {
        let _ = write!(out, " {key}=\"{}\"", escape(value));
    }
}

#[cfg(test)]
mod tests {
    use super::{XmlNode, escape};

    #[test]
    fn escapes_markup_characters() {
        assert_eq!(escape(r#"a & <b> "c" 'd'"#), "a &amp; &lt;b&gt; &quot;c&quot; &apos;d&apos;");
    }

    #[test]
    fn renders_nested_tree() {
        let node = XmlNode::element(
            "feed",
            vec![("xmlns", "urn:x".to_string())],
            vec![XmlNode::leaf("id", "1"), XmlNode::void("link", vec![("rel", "self".to_string())])],
        );
        assert_eq!(node.render(), r#"<feed xmlns="urn:x"><id>1</id><link rel="self"/></feed>"#);
    }

    #[test]
    fn render_document_prepends_declaration() {
        let node = XmlNode::leaf("id", "1");
        assert_eq!(node.render_document(), "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<id>1</id>");
    }

    #[test]
    fn escapes_text_and_attribute_values() {
        let node = XmlNode::element("a", vec![("k", "<v>".to_string())], vec![XmlNode::text("1 < 2")]);
        assert_eq!(node.render(), r#"<a k="&lt;v&gt;">1 &lt; 2</a>"#);
    }

    #[test]
    fn empty_element_still_closes_explicitly() {
        assert_eq!(XmlNode::element("author", Vec::new(), Vec::new()).render(), "<author></author>");
    }
}
