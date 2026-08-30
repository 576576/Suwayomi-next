//! OPDS XML model — mirrors `opds/model/*Xml.kt`.
//!
//! Feed/Entry/Link render to Atom XML with the OPDS namespaces declared on
//! the root `<feed>` element.

use crate::constants::*;
use crate::xml::XmlWriter;

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

impl Entry {
    pub fn render(&self, w: &mut XmlWriter) {
        w.element("entry", &[], |w| {
            w.leaf("id", &self.id);
            w.leaf("title", &self.title);
            if let Some(s) = &self.summary {
                w.element("summary", &[("type", "text")], |w| w.raw_text(&s.value));
            }
            if let Some(c) = &self.content {
                w.element("content", &[("type", "text")], |w| w.raw_text(&c.value));
            }
            for a in &self.authors {
                w.element("author", &[], |w| {
                    w.leaf("name", &a.name);
                    if let Some(u) = &a.uri {
                        w.leaf("uri", u);
                    }
                });
            }
            for c in &self.categories {
                let mut attrs: Vec<(&str, String)> = vec![("term", c.term.clone()), ("label", c.label.clone())];
                if let Some(s) = &c.scheme {
                    attrs.push(("scheme", s.clone()));
                }
                let attrs_ref: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
                w.void("category", &attrs_ref);
            }
            for l in &self.links {
                render_link(w, l);
            }
            w.leaf("updated", &self.updated);
            if let Some(v) = &self.extent {
                w.leaf("dc:extent", v);
            }
            if let Some(v) = &self.format {
                w.leaf("dc:format", v);
            }
            if let Some(v) = &self.language {
                w.leaf("dc:language", v);
            }
            if let Some(v) = &self.publisher {
                w.leaf("dc:publisher", v);
            }
            if let Some(v) = &self.issued {
                w.leaf("dc:issued", v);
            }
        });
    }
}

fn render_link(w: &mut XmlWriter, l: &Link) {
    let mut attrs: Vec<(&str, String)> = vec![("rel", l.rel.clone()), ("href", l.href.clone())];
    if let Some(t) = &l.link_type {
        attrs.push(("type", t.clone()));
    }
    if let Some(t) = &l.title {
        attrs.push(("title", t.clone()));
    }
    if let Some(g) = &l.facet_group {
        attrs.push(("opds:facetGroup", g.clone()));
    }
    if let Some(a) = l.active_facet {
        attrs.push(("opds:activeFacet", a.to_string()));
    }
    if let Some(c) = l.thr_count {
        attrs.push(("thr:count", c.to_string()));
    }
    if let Some(n) = l.length {
        attrs.push(("length", n.to_string()));
    }
    if let Some(c) = l.pse_count {
        attrs.push(("pse:count", c.to_string()));
    }
    if let Some(r) = l.pse_last_read {
        attrs.push(("pse:lastRead", r.to_string()));
    }
    if let Some(d) = &l.pse_last_read_date {
        attrs.push(("pse:lastReadDate", d.clone()));
    }
    let attrs_ref: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    w.void("link", &attrs_ref);
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
    pub fn render(&self) -> String {
        let mut w = XmlWriter::new();
        w.declaration();
        w.element(
            "feed",
            &[
                ("xmlns", NS_ATOM),
                ("xmlns:xsd", NS_XML_SCHEMA),
                ("xmlns:xsi", NS_XML_SCHEMA_INSTANCE),
                ("xmlns:opds", NS_OPDS),
                ("xmlns:dc", NS_DUBLIN_CORE),
                ("xmlns:pse", NS_PSE),
                ("xmlns:opensearch", NS_OPENSEARCH),
                ("xmlns:thr", NS_THREAD),
            ],
            |w| {
                w.leaf("id", &self.id);
                w.leaf("title", &self.title);
                if let Some(icon) = &self.icon {
                    w.leaf("icon", icon);
                }
                w.leaf("updated", &self.updated);
                w.element("author", &[], |w| {
                    w.leaf("name", &self.author.name);
                    if let Some(u) = &self.author.uri {
                        w.leaf("uri", u);
                    }
                });
                for l in &self.links {
                    render_link(w, l);
                }
                if let Some(t) = self.total_results {
                    w.leaf("opensearch:totalResults", &t.to_string());
                }
                if let Some(p) = self.items_per_page {
                    w.leaf("opensearch:itemsPerPage", &p.to_string());
                }
                if let Some(s) = self.start_index {
                    w.leaf("opensearch:startIndex", &s.to_string());
                }
                for e in &self.entries {
                    e.render(w);
                }
            },
        );
        w.finish()
    }
}
