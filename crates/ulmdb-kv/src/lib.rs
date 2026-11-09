//! Ordered byte-key B+tree with checked, byte-encoded copy-on-write pages.
//! Storage ownership and snapshot pinning belong to the supplied store.
mod bulk;
mod compact;
mod cursor;
mod graph;
mod ordered_read;
pub mod ordering;
mod page;
mod page_buffer;
mod read;
mod reader;
mod write;

pub use bulk::{
    BulkBuild, InOrderBuilder, SortedBuilder, build_sorted, build_sorted_with, rebuild_inorder,
};
pub use cursor::{Cursor, CursorPosition, InOrderPosition};
pub use graph::{
    TreeAudit, audit_tree, audit_tree_structure, audit_tree_with, audit_tree_with_leaf_visitor,
};
pub use ordered_read::{get_pair_with, get_with};
pub use page::{PAGE_SIZE, validate_page, validate_page_structure, validate_page_with};
pub use read::{get, get_pair, height};
pub use reader::Reader;
pub use write::ordered::{delete_with, put_with};
pub use write::{delete, put};
pub use write::{BatchUpsert, upsert_sorted, upsert_sorted_with};
pub use write::classify_dense_v1_leaf;
pub use write::{put_reserved, put_reserved_with};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod patch_tests;

/// Actual compiled eight-byte comparison strategy; observe outside hot loops.
pub const fn word_search_enabled() -> bool {
    cfg!(feature = "word-search")
}

/// Experimental cold typed object description; no reclaim authority.
pub mod object_graph;

/// Whether new eligible pages are emitted with the explicit version2 layout.
pub const fn compact_pages_enabled() -> bool {
    cfg!(feature = "compact-pages")
}
