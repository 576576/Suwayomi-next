//! h2-dump — Phase 7 migration tool.
//!
//! Reads a Suwayomi (Kotlin) H2 MVStore database file and emits a PostgreSQL
//! import script (INSERT statements against the `suwayomi` schema) so the Rust
//! server can adopt an existing library.
//!
//! Usage:
//!   h2-dump <h2-db-path> [output.sql]
//!
//! The H2 path may be the base name (e.g. `data/tachidesk`) or include the
//! `.mv.db` suffix. Output defaults to `<base>.suwayomi.sql`.

package h2dump

import java.io.File
import java.sql.Connection
import java.sql.DriverManager
import java.sql.ResultSet
import java.sql.Types

/** Kotlin-Exposed table names (H2 stores them upper-case by default). */
private val IGNORED_TABLES = setOf(
    "MIGRATION", "MIGRATIONS", "DE_NEONEW_MIGRATIONS", "FLYWAY_SCHEMA_HISTORY",
)

/**
 * FK-dependency-safe export order (a plain alphabetical order would insert
 * e.g. `category_manga` before `manga` and trip foreign keys).
 */
private val TABLE_ORDER = listOf(
    "EXTENSION", "SOURCE", "MANGA", "CHAPTER", "PAGE",
    "CATEGORY", "CATEGORY_MANGA", "CATEGORY_META", "CHAPTER_META",
    "MANGA_META", "SOURCE_META", "GLOBAL_META", "EXTENSION_STORE",
    "TRACK_RECORD", "TRACK_SEARCH",
)

private fun camelToSnake(name: String): String =
    name.replace(Regex("([a-z0-9])([A-Z])"), "$1_$2").lowercase()

private fun snake(s: String): String = s.lowercase()

private fun sqlString(v: String): String = "'" + v.replace("'", "''") + "'"

private fun sqlLiteral(rs: ResultSet, i: Int, type: Int): String {
    val v = rs.getObject(i) ?: return "NULL"
    return when (type) {
        Types.BOOLEAN, Types.BIT -> if (rs.getBoolean(i)) "TRUE" else "FALSE"
        Types.BIGINT, Types.INTEGER, Types.SMALLINT, Types.TINYINT,
        Types.DOUBLE, Types.FLOAT, Types.REAL, Types.NUMERIC, Types.DECIMAL -> rs.getObject(i).toString()
        Types.BINARY, Types.VARBINARY, Types.LONGVARBINARY -> {
            val bytes = rs.getBytes(i)
            "E'\\\\x" + bytes.joinToString("") { "%02x".format(it) } + "'"
        }
        else -> sqlString(v.toString())
    }
}

fun main(args: Array<String>) {
    if (args.isEmpty()) {
        System.err.println("usage: h2-dump <h2-db-path> [output.sql]")
        kotlin.system.exitProcess(1)
    }
    val dbArg = args[0].removeSuffix(".mv.db")
    val outFile = File(args.getOrElse(1) { "$dbArg.suwayomi.sql" })
    val url = "jdbc:h2:file:$dbArg"

    Class.forName("org.h2.Driver")
    val conn: Connection = DriverManager.getConnection(url, "sa", "")

    val tableNames = conn.createStatement().use { st ->
        st.executeQuery(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'PUBLIC' ORDER BY table_name"
        ).use { rs ->
            buildList {
                while (rs.next()) add(rs.getString(1))
            }
        }
    }
        .filterNot { it.uppercase() in IGNORED_TABLES }
        .sortedBy { t -> TABLE_ORDER.indexOf(t.uppercase()).let { if (it < 0) Int.MAX_VALUE else it } }

    val sb = StringBuilder()
    sb.appendLine("-- h2-dump: Suwayomi (Kotlin/H2) -> PostgreSQL import script")
    sb.appendLine("-- target schema: suwayomi (created by the Rust server migrations)")
    sb.appendLine("-- NOTE: run AFTER `suwayomi-server --migrate` so tables exist.")
    sb.appendLine()

    for (table in tableNames) {
        val tbl = snake(table)
        val columns = conn.createStatement().use { st ->
            st.executeQuery(
                "SELECT column_name, data_type FROM information_schema.columns WHERE table_schema = 'PUBLIC' AND table_name = '$table' ORDER BY ordinal_position"
            ).use { rs ->
                buildList {
                    while (rs.next()) add(rs.getString(1) to rs.getString(2))
                }
            }
        }
        val colNames = columns.map { (c, _) -> "\"${snake(c)}\"" }
        val colList = colNames.joinToString(", ")

        // idempotent per-table clear (safe on a fresh migrated DB)
        sb.appendLine("DELETE FROM \"suwayomi\".\"$tbl\";")

        val rows = conn.createStatement().use { st ->
            st.executeQuery("SELECT * FROM \"$table\"").use { rs ->
                buildList {
                    val md = rs.metaData
                    while (rs.next()) {
                        add((1..md.columnCount).map { i -> sqlLiteral(rs, i, md.getColumnType(i)) })
                    }
                }
            }
        }
        for (row in rows) {
            sb.appendLine("INSERT INTO \"suwayomi\".\"$tbl\" ($colList) VALUES (${row.joinToString(", ")});")
        }
        sb.appendLine()
    }

    outFile.parentFile?.mkdirs()
    outFile.writeText(sb.toString())
    println("h2-dump: wrote ${outFile.absolutePath} (${tableNames.size} tables, ${conn.metaData.databaseProductVersion})")
    conn.close()
}
