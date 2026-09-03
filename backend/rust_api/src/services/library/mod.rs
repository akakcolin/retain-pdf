//! Library domain services.
//!
//! Routes talk to this domain only through `services::library_api`.

mod assets;
mod books;
mod collections;
mod conversations;
mod documents;
mod favorites;
mod media;
mod search;
mod translate;

use std::path::Path;

use crate::db::Db;

pub use assets::{load_asset, store_asset, AssetDownload};
pub use books::{delete_library_book, delete_library_books, get_library_book, list_library_books};
pub use collections::{
    add_collection_documents, create_collection, delete_collection, list_collections,
    patch_collection, remove_collection_document,
};
pub use conversations::{
    append_message, create_conversation, delete_conversation, get_conversation, list_conversations,
    patch_conversation, visible_path_messages,
};
pub use documents::{delete_document, get_document, list_documents, patch_document};
pub use favorites::{create_favorite, delete_favorite, list_favorites, patch_favorite};
pub use media::{document_cover, document_source_pdf, document_thumbnail, DocumentFileDownload};
pub use search::search_blocks;
pub use translate::translate_document;

pub struct LibraryDeps<'a> {
    pub db: &'a Db,
    pub data_root: &'a Path,
    pub output_root: &'a Path,
    pub downloads_dir: &'a Path,
    /// Used for document cover/thumbnail generation via derived_artifacts.
    pub scripts_dir: &'a Path,
    pub python_bin: &'a str,
}
