//! Serialization golden tests — JSON field names must match Kotlin jackson
//! output exactly (camelCase), enum values match Kotlin names, and `memo`
//! is excluded from serialization (@JsonIgnore).

use serde_json::json;
use suwayomi_core::models::{
    CategoryDataClass, ChapterDataClass, IncludeOrExclude, MangaDataClass, MangaStatus, PageDataClass, UpdateStrategy,
};

#[test]
fn manga_serialization_matches_kotlin_field_names() {
    let manga = MangaDataClass {
        id: 1,
        source_id: "123".into(),
        url: "/manga/1".into(),
        title: "Test".into(),
        thumbnail_url: Some("https://example.com/thumb.jpg".into()),
        thumbnail_url_last_fetched: 123,
        initialized: true,
        artist: None,
        author: Some("Author".into()),
        description: None,
        genre: vec!["Action".into(), "Adventure".into()],
        status: MangaStatus::Ongoing,
        in_library: true,
        in_library_at: 456,
        source: None,
        real_url: None,
        last_fetched_at: Some(100),
        chapters_last_fetched_at: None,
        update_strategy: UpdateStrategy::AlwaysUpdate,
        fresh_data: false,
        unread_count: Some(3),
        download_count: None,
        chapter_count: Some(5),
        last_read_at: Some(200),
        last_chapter_read: None,
        age: Some(50),
        chapters_age: None,
        trackers: None,
        last_modified_at: 0,
        version: 0,
        memo: serde_json::Value::Null,
    };

    let v = serde_json::to_value(&manga).expect("serialize");
    let obj = v.as_object().expect("object");

    // camelCase keys as produced by Kotlin jackson
    for key in [
        "id",
        "sourceId",
        "url",
        "title",
        "thumbnailUrl",
        "thumbnailUrlLastFetched",
        "initialized",
        "author",
        "genre",
        "status",
        "inLibrary",
        "inLibraryAt",
        "lastFetchedAt",
        "chaptersLastFetchedAt",
        "updateStrategy",
        "freshData",
        "unreadCount",
        "chapterCount",
        "lastReadAt",
        "age",
        "chaptersAge",
        "lastModifiedAt",
        "version",
    ] {
        assert!(obj.contains_key(key), "missing key {key}: {v}");
    }

    // @JsonIgnore: memo must NOT appear
    assert!(!obj.contains_key("memo"), "memo must be ignored: {v}");
    assert_eq!(obj["status"], "ONGOING");
    assert_eq!(obj["updateStrategy"], "ALWAYS_UPDATE");
    assert_eq!(obj["genre"], json!(["Action", "Adventure"]));
}

#[test]
fn chapter_serialization_matches_kotlin_field_names() {
    let chapter = ChapterDataClass {
        id: 1,
        url: "/chapter/1".into(),
        name: "Ch 1".into(),
        upload_date: 100,
        chapter_number: 1.5,
        scanlator: None,
        manga_id: 1,
        read: true,
        bookmarked: false,
        last_page_read: 3,
        last_read_at: 0,
        index: 1,
        fetched_at: 0,
        real_url: None,
        downloaded: false,
        page_count: -1,
        last_modified_at: 0,
        version: 0,
        memo: serde_json::Value::Null,
    };
    let v = serde_json::to_value(&chapter).expect("serialize");
    let obj = v.as_object().expect("object");
    for key in [
        "id",
        "url",
        "name",
        "uploadDate",
        "chapterNumber",
        "scanlator",
        "mangaId",
        "read",
        "bookmarked",
        "lastPageRead",
        "lastReadAt",
        "index",
        "fetchedAt",
        "realUrl",
        "downloaded",
        "pageCount",
    ] {
        assert!(obj.contains_key(key), "missing key {key}");
    }
    assert!(!obj.contains_key("memo"));
    assert_eq!(obj["chapterNumber"], json!(1.5));
}

#[test]
fn page_and_category_serialization() {
    let page = PageDataClass { index: 0, image_url: "https://x/i.jpg".into() };
    let v = serde_json::to_value(&page).expect("serialize");
    let obj = v.as_object().expect("object");
    assert_eq!(obj["imageUrl"], "https://x/i.jpg");

    let cat = CategoryDataClass {
        id: 1,
        order: 0,
        name: "Default".into(),
        default: false,
        include_in_update: IncludeOrExclude::Unset,
        include_in_download: IncludeOrExclude::Include,
        version: 0,
        uid: 0,
        last_modified_at: 0,
    };
    let v = serde_json::to_value(&cat).expect("serialize");
    let obj = v.as_object().expect("object");
    assert_eq!(obj["includeInUpdate"], "Unset");
    assert_eq!(obj["includeInDownload"], "Include");
}

#[test]
fn enum_db_values_match_kotlin() {
    // MangaStatus: UNKNOWN=0, ONGOING=1, COMPLETED=2, LICENSED=3, PUBLISHING_FINISHED=4, CANCELLED=5, ON_HIATUS=6
    assert_eq!(MangaStatus::Unknown.to_i32(), 0);
    assert_eq!(MangaStatus::Ongoing.to_i32(), 1);
    assert_eq!(MangaStatus::Completed.to_i32(), 2);
    assert_eq!(MangaStatus::Cancelled.to_i32(), 5);
    assert_eq!(MangaStatus::OnHiatus.to_i32(), 6);
    assert_eq!(MangaStatus::from_i32(999), MangaStatus::Unknown);

    // IncludeOrExclude: EXCLUDE=0, INCLUDE=1, UNSET=-1
    assert_eq!(IncludeOrExclude::Exclude.to_i32(), 0);
    assert_eq!(IncludeOrExclude::Include.to_i32(), 1);
    assert_eq!(IncludeOrExclude::Unset.to_i32(), -1);
    assert_eq!(IncludeOrExclude::from_i32(42), IncludeOrExclude::Unset);

    // UpdateStrategy DB strings
    assert_eq!(UpdateStrategy::AlwaysUpdate.to_db(), "ALWAYS_UPDATE");
    assert_eq!(UpdateStrategy::OnlyFetchOnce.to_db(), "ONLY_FETCH_ONCE");
    assert_eq!(UpdateStrategy::from_db("ONLY_FETCH_ONCE"), UpdateStrategy::OnlyFetchOnce);
    assert_eq!(UpdateStrategy::from_db("garbage"), UpdateStrategy::AlwaysUpdate);
}

#[test]
fn genre_list_parsing_matches_kotlin() {
    use suwayomi_core::models::manga::to_genre_list;
    assert_eq!(to_genre_list(Some("Action, Adventure,  Comedy ")), vec!["Action", "Adventure", "Comedy"]);
    assert_eq!(to_genre_list(None), Vec::<String>::new());
    assert_eq!(to_genre_list(Some("")), Vec::<String>::new());
}
