//! Stages the Oliphaunt (embedded PostgreSQL) native runtime artifacts into
//! `OUT_DIR` and emits `OLIPHAUNT_RESOURCES_DIR` for
//! `register_build_resources!()`. See `crates/suwayomi-core/src/db/manager.rs`.

fn main() {
    oliphaunt_build::configure();
}
