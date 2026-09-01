@file:Suppress("ktlint:standard:property-naming")

package eu.kanade.tachiyomi.source.model

import kotlinx.serialization.json.JsonObject
import java.io.Serializable

interface SChapter : Serializable {
    var url: String

    var name: String

    var chapter_number: Float

    /** New lib spelling (`chapterNumber`); see [chapter_number]. */
    var chapterNumber: Float

    var scanlator: String?

    var date_upload: Long

    /** New lib spelling (`dateUpload`); see [date_upload]. */
    var dateUpload: Long

    /**
     * Extra metadata associated with the chapter.
     *
     * The JSON object is not visible to users and intended for internal or source-specific
     * purposes. Apps may define their own namespaced keys (e.g., `"mihon.*"`) for sources to populate.
     *
     * This allows apps to attach and ask for custom information without affecting the visible
     * chapter data.
     *
     * @since tachiyomix 1.6
     */
    var memo: JsonObject

    fun copyFrom(other: SChapter) {
        name = other.name
        url = other.url
        date_upload = other.date_upload
        dateUpload = other.dateUpload
        chapter_number = other.chapter_number
        chapterNumber = other.chapterNumber
        scanlator = other.scanlator
    }

    companion object {
        fun create(): SChapter = SChapterImpl()
    }
}
