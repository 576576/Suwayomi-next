//! OPDS 1.2 constants — mirrors `opds/constants/OpdsConstants.kt`.

pub const NS_ATOM: &str = "http://www.w3.org/2005/Atom";
pub const NS_XML_SCHEMA: &str = "http://www.w3.org/2001/XMLSchema";
pub const NS_XML_SCHEMA_INSTANCE: &str = "http://www.w3.org/2001/XMLSchema-instance";
pub const NS_OPDS: &str = "http://opds-spec.org/2010/catalog";
pub const NS_DUBLIN_CORE: &str = "http://purl.org/dc/terms/";
pub const NS_PSE: &str = "http://vaemendis.net/opds-pse/ns";
pub const NS_OPENSEARCH: &str = "http://a9.com/-/spec/opensearch/1.1/";
pub const NS_THREAD: &str = "http://purl.org/syndication/thread/1.0";

// Link relations
pub const REL_ACQUISITION: &str = "http://opds-spec.org/acquisition";
pub const REL_ACQUISITION_OPEN_ACCESS: &str = "http://opds-spec.org/acquisition/open-access";
pub const REL_IMAGE: &str = "http://opds-spec.org/image";
pub const REL_IMAGE_THUMBNAIL: &str = "http://opds-spec.org/image/thumbnail";
pub const REL_SELF: &str = "self";
pub const REL_START: &str = "start";
pub const REL_SUBSECTION: &str = "subsection";
pub const REL_ALTERNATE: &str = "alternate";
pub const REL_FACET: &str = "http://opds-spec.org/facet";
pub const REL_SEARCH: &str = "search";
pub const REL_PREV: &str = "previous";
pub const REL_NEXT: &str = "next";
pub const REL_FIRST: &str = "first";
pub const REL_LAST: &str = "last";
pub const REL_PSE_STREAM: &str = "http://vaemendis.net/opds-pse/stream";
pub const REL_CRAWLABLE: &str = "http://opds-spec.org/crawlable";
pub const REL_SORT_NEW: &str = "http://opds-spec.org/sort/new";
pub const REL_SORT_POPULAR: &str = "http://opds-spec.org/sort/popular";

// Media types
pub const TYPE_ATOM_FEED_NAVIGATION: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
pub const TYPE_ATOM_FEED_ACQUISITION: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition";
pub const TYPE_ATOM_ENTRY_OPDS: &str = "application/atom+xml;type=entry;profile=opds-catalog";
pub const TYPE_OPENSEARCH_DESCRIPTION: &str = "application/opensearchdescription+xml";
pub const TYPE_IMAGE_JPEG: &str = "image/jpeg";
pub const TYPE_IMAGE_PNG: &str = "image/png";
pub const TYPE_TEXT_HTML: &str = "text/html";
pub const TYPE_CBZ: &str = "application/vnd.comicbook+zip";
pub const TYPE_EPUB: &str = "application/epub+zip";

/// MIME types for the OPDS responses.
pub const MIME_OPDS_CATALOG: &str = "application/xml;profile=opds-catalog;charset=UTF-8";
pub const MIME_OPENSEARCH: &str = "application/opensearchdescription+xml;charset=UTF-8";

/// How many items per OPDS feed page (mirrors `opdsItemsPerPage` default).
pub const ITEMS_PER_PAGE: usize = 50;
