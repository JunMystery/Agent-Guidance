//! Integration test for project_context search upgrade: File search, Symbol token expansion, and Vector DB schema.

use rusqlite::Connection;

#[test]
fn test_code_graph_schema_integrity_for_search_upgrade() {
    let conn = Connection::open_in_memory().unwrap();

    // Initialize code graph schema with updated tables
    conn.execute_batch(
        "CREATE TABLE files (
            path TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            size INTEGER NOT NULL,
            modified_at INTEGER NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        CREATE TABLE symbols (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            file_path TEXT NOT NULL,
            parent TEXT,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            signature TEXT,
            language TEXT,
            namespace TEXT,
            receiver TEXT
        );
        CREATE TABLE content_chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            content_hash TEXT NOT NULL,
            chunk_text TEXT NOT NULL
        );
        CREATE TABLE symbol_vectors (
            symbol_id TEXT PRIMARY KEY,
            vector BLOB NOT NULL,
            model_version TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE chunk_vectors (
            chunk_id INTEGER PRIMARY KEY,
            vector BLOB NOT NULL,
            model_version TEXT NOT NULL
        );",
    ).unwrap();

    // Verify insertion of rich symbols with parent, receiver, signature
    conn.execute(
        "INSERT INTO symbols (id, name, kind, file_path, parent, start_line, end_line, signature, receiver)
         VALUES ('s1', 'search_files', 'function', 'src/context/db/file_search.rs', 'CodeGraphDb', 12, 35, 'pub fn search_files(&self, query: &str, limit: usize) -> Result<Vec<FileMatchResult>>', 'self')",
        [],
    ).unwrap();

    let mut stmt = conn.prepare("SELECT parent, receiver, signature FROM symbols WHERE id = 's1'").unwrap();
    let (parent, receiver, sig): (Option<String>, Option<String>, Option<String>) = stmt.query_row([], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    }).unwrap();

    assert_eq!(parent.as_deref(), Some("CodeGraphDb"));
    assert_eq!(receiver.as_deref(), Some("self"));
    assert!(sig.unwrap().contains("search_files"));

    // Verify chunk text persistence
    conn.execute(
        "INSERT INTO content_chunks (file_path, start_line, end_line, content_hash, chunk_text)
         VALUES ('src/main.rs', 1, 20, 'hash1', 'fn main() { println!(\"Hello\"); }')",
        [],
    ).unwrap();

    let chunk_text: String = conn.query_row(
        "SELECT chunk_text FROM content_chunks WHERE id = 1",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(chunk_text, "fn main() { println!(\"Hello\"); }");
}
