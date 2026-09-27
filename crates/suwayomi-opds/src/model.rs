//! OPDS XML model — mirrors `opds/model/*Xml.kt`.
//!
//! Feed/Entry/Link render to Atom XML with the OPDS namespaces declared on
//! the root `<feed>` element.

use crate::constants::{
    NS_ATOM, NS_DUBLIN_CORE, NS_OPDS, NS_OPENSEARCH, NS_PSE, NS_THREAD, NS_XML_SCHEMA, NS_XML_SCHEMA_INSTANCE,
};
use crate::xml::{Attrs, XmlNode};

#[derive(Debug, Clone, Default)]
pub struct Link {
    pub rel: String,
    pub href: String,
    pub link_type: Option<String>,
    pub title: Option<String>,
    pub facet_group: Option<String>,
    pub active_facet: Option<bool>,
    pub thr_count: Option<usize>,
    pub length: Option<u64>,
    pub pse_count: Option<usize>,
    pub pse_last_read: Option<usize>,
    pub pse_last_read_date: Option<String>,
}

impl Link {
    pub fn new(rel: impl Into<String>, href: impl Into<String>, link_type: impl Into<String>) -> Self {
        Self { rel: rel.into(), href: href.into(), link_type: Some(link_type.into()), ..Default::default() }
    }

    /// 转成不可变节点。
    ///
    /// 可选属性各自是一个 `Option`，用 `extend` 收进属性表——
    /// 没有一处 `if let Some(..) { attrs.push(..) }`，顺序也一眼可见。
    pub fn to_node(&self) -> XmlNode {
        let mut attrs: Attrs = vec![("rel", self.rel.clone()), ("href", self.href.clone())];
        attrs.extend(self.link_type.as_ref().map(|v| ("type", v.clone())));
        attrs.extend(self.title.as_ref().map(|v| ("title", v.clone())));
        attrs.extend(self.facet_group.as_ref().map(|v| ("opds:facetGroup", v.clone())));
        attrs.extend(self.active_facet.map(|v| ("opds:activeFacet", v.to_string())));
        attrs.extend(self.thr_count.map(|v| ("thr:count", v.to_string())));
        attrs.extend(self.length.map(|v| ("length", v.to_string())));
        attrs.extend(self.pse_count.map(|v| ("pse:count", v.to_string())));
        attrs.extend(self.pse_last_read.map(|v| ("pse:lastRead", v.to_string())));
        attrs.extend(self.pse_last_read_date.as_ref().map(|v| ("pse:lastReadDate", v.clone())));
        XmlNode::void("link", attrs)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Author {
    pub name: String,
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Category {
    pub scheme: Option<String>,
    pub term: String,
    pub label: String,
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct Content {
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub updated: String,
    pub summary: Option<Summary>,
    pub content: Option<Content>,
    pub links: Vec<Link>,
    pub authors: Vec<Author>,
    pub categories: Vec<Category>,
    // Dublin Core
    pub extent: Option<String>,
    pub format: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub issued: Option<String>,
}

impl Author {
    pub fn to_node(&self) -> XmlNode {
        let mut children = vec![XmlNode::leaf("name", &self.name)];
        children.extend(self.uri.as_ref().map(|uri| XmlNode::leaf("uri", uri)));
        XmlNode::element("author", Vec::new(), children)
    }
}

impl Category {
    pub fn to_node(&self) -> XmlNode {
        let mut attrs: Attrs = vec![("term", self.term.clone()), ("label", self.label.clone())];
        attrs.extend(self.scheme.as_ref().map(|s| ("scheme", s.clone())));
        XmlNode::void("category", attrs)
    }
}

impl Entry {
    /// 转成不可变节点。子节点顺序与 Atom/OPDS 期望的一致，全部由 `extend` 拼装。
    pub fn to_node(&self) -> XmlNode {
        let mut children = vec![XmlNode::leaf("id", &self.id), XmlNode::leaf("title", &self.title)];
        children.extend(
            self.summary.as_ref().map(|s| {
                XmlNode::element("summary", vec![("type", "text".to_string())], vec![XmlNode::text(&s.value)])
            }),
        );
        children.extend(
            self.content.as_ref().map(|c| {
                XmlNode::element("content", vec![("type", "text".to_string())], vec![XmlNode::text(&c.value)])
            }),
        );
        children.extend(self.authors.iter().map(Author::to_node));
        children.extend(self.categories.iter().map(Category::to_node));
        children.extend(self.links.iter().map(Link::to_node));
        children.push(XmlNode::leaf("updated", &self.updated));
        // Dublin Core 五个字段是一张「名字 → 可选值」的表，`filter_map` 一次收齐。
        children.extend(
            [
                ("dc:extent", self.extent.as_deref()),
                ("dc:format", self.format.as_deref()),
                ("dc:language", self.language.as_deref()),
                ("dc:publisher", self.publisher.as_deref()),
                ("dc:issued", self.issued.as_deref()),
            ]
            .into_iter()
            .filter_map(|(name, value)| value.map(|v| XmlNode::leaf(name, v))),
        );
        XmlNode::element("entry", Vec::new(), children)
    }
}

/// Feed document (root `<feed>` in the Atom namespace with OPDS namespaces).
pub struct Feed {
    pub id: String,
    pub title: String,
    pub updated: String,
    pub icon: Option<String>,
    pub author: Author,
    pub links: Vec<Link>,
    pub entries: Vec<Entry>,
    pub total_results: Option<u64>,
    pub items_per_page: Option<usize>,
    pub start_index: Option<usize>,
}

impl Feed {
    /// 转成不可变节点：命名空间在根元素上一次声明。
    pub fn to_node(&self) -> XmlNode {
        let namespaces: Attrs = [
            ("xmlns", NS_ATOM),
            ("xmlns:xsd", NS_XML_SCHEMA),
            ("xmlns:xsi", NS_XML_SCHEMA_INSTANCE),
            ("xmlns:opds", NS_OPDS),
            ("xmlns:dc", NS_DUBLIN_CORE),
            ("xmlns:pse", NS_PSE),
            ("xmlns:opensearch", NS_OPENSEARCH),
            ("xmlns:thr", NS_THREAD),
        ]
        .into_iter()
        .map(|(key, value)| (key, value.to_string()))
        .collect();

        let mut children = vec![XmlNode::leaf("id", &self.id), XmlNode::leaf("title", &self.title)];
        children.extend(self.icon.as_ref().map(|icon| XmlNode::leaf("icon", icon)));
        children.push(XmlNode::leaf("updated", &self.updated));
        children.push(self.author.to_node());
        children.extend(self.links.iter().map(Link::to_node));
        children.extend(
            [
                ("opensearch:totalResults", self.total_results.map(|v| v.to_string())),
                ("opensearch:itemsPerPage", self.items_per_page.map(|v| v.to_string())),
                ("opensearch:startIndex", self.start_index.map(|v| v.to_string())),
            ]
            .into_iter()
            .filter_map(|(name, value)| value.map(|v| XmlNode::leaf(name, &v))),
        );
        children.extend(self.entries.iter().map(Entry::to_node));
        XmlNode::element("feed", namespaces, children)
    }

    pub fn render(&self) -> String {
        self.to_node().render_document()
    }
}
