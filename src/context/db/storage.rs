use anyhow::Result;
use rusqlite::params;
use std::time::{SystemTime, UNIX_EPOCH};

use super::CodeGraphDb;
use super::sanitize_fts5_query;

fn escape_like_literal(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

impl CodeGraphDb {
    #[cfg(test)]
    pub fn get_file_content_hash(&self, path: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT content_hash FROM files WHERE path = ?")?;
        let mut rows = stmt.query(params![path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_file_metadata(&self, path: &str) -> Result<Option<(String, u64, i64)>> {
        let mut stmt = self.conn.prepare("SELECT content_hash, size, modified_at FROM files WHERE path = ?")?;
        let mut rows = stmt.query(params![path])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get::<_, i64>(1)? as u64, row.get(2)?)))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_file(&self, path: &str, content_hash: &str, size: u64, modified_at: i64) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn.execute(
            "INSERT INTO files (path, content_hash, size, modified_at, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET
                content_hash = ?2,
                size = ?3,
                modified_at = ?4,
                indexed_at = ?5",
            params![path, content_hash, size as i64, modified_at, now],
        )?;
        Ok(())
    }

    pub fn clear_file_data(&self, file_path: &str) -> Result<()> {
        // Cascade will automatically clear symbols, symbol_edges, symbol_vectors, content_chunks, chunk_vectors
        self.conn.execute("DELETE FROM symbols WHERE file_path = ?", params![file_path])?;
        self.conn.execute("DELETE FROM content_chunks WHERE file_path = ?", params![file_path])?;
        let prefix = format!("{}::%", escape_like_literal(file_path));
        let _ = self.conn.execute(
            "DELETE FROM data_flow_edges WHERE caller_symbol_id LIKE ?1 ESCAPE '\\' OR callee_symbol_id LIKE ?1 ESCAPE '\\'",
            params![prefix],
        );
        Ok(())
    }

    pub fn delete_file(&self, file_path: &str) -> Result<()> {
        let _ = self.clear_file_data(file_path);
        self.conn.execute("DELETE FROM files WHERE path = ?", params![file_path])?;
        Ok(())
    }

    pub fn prune_orphaned_files(&self, project_path: &std::path::Path) -> Result<usize> {
        let mut stmt = self.conn.prepare("SELECT path FROM files")?;
        let recorded_paths: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut pruned = 0;
        for path in recorded_paths {
            if (crate::context::exclusion::is_excluded_path(&path) || !project_path.join(&path).exists())
                && self.delete_file(&path).is_ok()
            {
                pruned += 1;
            }
        }

        if pruned > 0 {
            let _ = self.conn.execute_batch(
                "PRAGMA incremental_vacuum;
                 PRAGMA wal_checkpoint(PASSIVE);",
            );
        }

        Ok(pruned)
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg(test)]
    pub fn insert_symbol(
        &self, id: &str, name: &str, kind: &str, file_path: &str,
        parent: Option<&str>, start_line: usize, end_line: usize, signature: Option<&str>,
    ) -> Result<()> {
        self.insert_symbol_full(id, name, kind, file_path, parent, start_line, end_line, signature, None, None, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_symbol_full(
        &self, id: &str, name: &str, kind: &str, file_path: &str,
        parent: Option<&str>, start_line: usize, end_line: usize, signature: Option<&str>,
        language: Option<&str>, namespace: Option<&str>, receiver: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO symbols (id, name, kind, file_path, parent, start_line, end_line, signature, language, namespace, receiver)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                file_path = excluded.file_path,
                parent = excluded.parent,
                start_line = excluded.start_line,
                end_line = excluded.end_line,
                signature = excluded.signature,
                language = excluded.language,
                namespace = excluded.namespace,
                receiver = excluded.receiver",
            params![id, name, kind, file_path, parent, start_line as i64, end_line as i64, signature, language.unwrap_or("unknown"), namespace, receiver],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn insert_edge(&self, source_id: &str, target_id: &str, edge_type: &str, weight: f64) -> Result<()> {
        self.insert_edge_full(source_id, target_id, edge_type, weight, 1.0, "symbol", None)
    }

    pub fn insert_edge_full(
        &self, source_id: &str, target_id: &str, edge_type: &str,
        weight: f64, confidence: f64, category: &str, call_line: Option<usize>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO symbol_edges (source_id, target_id, edge_type, weight, confidence, category, call_line)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![source_id, target_id, edge_type, weight, confidence, category, call_line.map(|l| l as i64)],
        )?;
        Ok(())
    }

    pub fn insert_chunk(
        &self,
        file_path: &str,
        start_line: usize,
        end_line: usize,
        content_hash: &str,
        chunk_text: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO content_chunks (file_path, start_line, end_line, content_hash, chunk_text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                file_path,
                start_line as i64,
                end_line as i64,
                content_hash,
                chunk_text
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn search_content_fts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, usize, usize, String)>> {
        let safe_query = sanitize_fts5_query(query);
        if safe_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            "SELECT c.file_path, c.start_line, c.end_line, snippet(content_fts, 1, '»', '«', '...', 15)
             FROM content_chunks c
             JOIN content_fts f ON c.id = f.rowid
             WHERE content_fts MATCH ?
             ORDER BY bm25(content_fts)
             LIMIT ?",
        )?;

        let rows = stmt.query_map(params![safe_query, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn search_related_symbols(&self, symbol_name: &str) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s1.name, e.edge_type, s2.name
             FROM symbol_edges e
             JOIN symbols s1 ON e.source_id = s1.id
             JOIN symbols s2 ON e.target_id = s2.id
             WHERE s1.name LIKE ?1 ESCAPE '\\' OR s2.name LIKE ?1 ESCAPE '\\'
             LIMIT 20",
        )?;

        let pattern = format!("%{}%", escape_like_literal(symbol_name.trim()));
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Count incoming dependencies for a specific file (how many other files import or call symbols in this file)
    pub fn count_incoming_dependencies(&self, file_path: &str) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(DISTINCT s1.file_path)
             FROM symbol_edges e
             JOIN symbols s1 ON e.source_id = s1.id
             JOIN symbols s2 ON e.target_id = s2.id
             WHERE s2.file_path = ?1 AND s1.file_path != ?1",
        )?;
        let count: i64 = stmt.query_row(params![file_path], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// Get list of other files that depend on symbols defined in this file
    pub fn get_incoming_dependent_files(&self, file_path: &str, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT s1.file_path
             FROM symbol_edges e
             JOIN symbols s1 ON e.source_id = s1.id
             JOIN symbols s2 ON e.target_id = s2.id
             WHERE s2.file_path = ?1 AND s1.file_path != ?1
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![file_path, limit as i64], |r| r.get(0))?;
        let mut files = Vec::new();
        for r in rows {
            files.push(r?);
        }
        Ok(files)
    }

    /// Retrieve precomputed embedding from content-addressable cache
    pub fn get_cached_embedding(&self, passage_hash: &str) -> Result<Option<Vec<f32>>> {
        let mut stmt = self.conn.prepare("SELECT vector FROM embedding_cache WHERE passage_hash = ?")?;
        let mut rows = stmt.query(params![passage_hash])?;
        if let Some(row) = rows.next()? {
            let bytes: Vec<u8> = row.get(0)?;
            Ok(Some(super::bytes_to_f32_vec(&bytes)))
        } else {
            Ok(None)
        }
    }

    /// Store computed embedding into content-addressable cache
    pub fn store_cached_embedding(&self, passage_hash: &str, vector: &[f32], model_version: &str) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let bytes: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
        self.conn.execute(
            "INSERT OR REPLACE INTO embedding_cache (passage_hash, vector, model_version, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![passage_hash, bytes, model_version, now],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "storage_test_{}_{}_{:?}",
            label,
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn test_upsert_symbol_preserves_fts5_integrity_and_edges() {
        let dir = temp_db_path("fts_upsert");
        let _ = std::fs::remove_dir_all(&dir);
        let db = CodeGraphDb::open(&dir.join("graph.db")).unwrap();

        db.upsert_file("src/a.rs", "h1", 10, 1).unwrap();
        db.insert_symbol("src/a.rs::fn::caller::L1", "caller", "function", "src/a.rs", None, 1, 5, Some("fn caller()")).unwrap();
        db.insert_symbol("src/a.rs::fn::target::L10", "target", "function", "src/a.rs", None, 10, 15, Some("fn target()")).unwrap();
        db.insert_edge("src/a.rs::fn::caller::L1", "src/a.rs::fn::target::L10", "calls", 1.0).unwrap();

        // Re-inserting existing symbol id must update in place without corrupting FTS5 or cascading edge deletion
        db.insert_symbol("src/a.rs::fn::target::L10", "target_renamed", "function", "src/a.rs", None, 10, 20, Some("fn target_renamed()")).unwrap();

        db.conn
            .execute("INSERT INTO symbols_fts(symbols_fts) VALUES('integrity-check')", [])
            .expect("symbols_fts integrity check must pass after symbol upsert");

        let related = db.search_related_symbols("target_renamed").unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].0, "caller");
        assert_eq!(related[0].2, "target_renamed");

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_like_wildcards_are_escaped_in_clear_and_search() {
        let dir = temp_db_path("like_escape");
        let _ = std::fs::remove_dir_all(&dir);
        let db = CodeGraphDb::open(&dir.join("graph.db")).unwrap();

        db.upsert_file("src/a_b.rs", "h1", 10, 1).unwrap();
        db.upsert_file("src/axb.rs", "h2", 10, 1).unwrap();

        db.insert_symbol("src/a_b.rs::fn::foo_bar::L1", "foo_bar", "function", "src/a_b.rs", None, 1, 5, None).unwrap();
        db.insert_symbol("src/axb.rs::fn::fooXbar::L1", "fooXbar", "function", "src/axb.rs", None, 1, 5, None).unwrap();
        db.insert_symbol("src/axb.rs::fn::sink::L10", "sink", "function", "src/axb.rs", None, 10, 15, None).unwrap();

        db.insert_edge("src/a_b.rs::fn::foo_bar::L1", "src/axb.rs::fn::sink::L10", "calls", 1.0).unwrap();
        db.insert_edge("src/axb.rs::fn::fooXbar::L1", "src/axb.rs::fn::sink::L10", "calls", 1.0).unwrap();

        db.conn.execute(
            "INSERT INTO data_flow_edges (caller_symbol_id, callee_symbol_id, source_variable, target_parameter, arg_index, call_line)
             VALUES (?1, ?2, 'x', 'y', 0, 2)",
            params!["src/axb.rs::fn::fooXbar::L1", "src/axb.rs::fn::sink::L10"],
        ).unwrap();

        // Searching with '_' must NOT match 'fooXbar'
        let matches = db.search_related_symbols("foo_bar").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "foo_bar");

        // Clearing 'src/a_b.rs' must NOT delete data_flow_edges for 'src/axb.rs'
        db.clear_file_data("src/a_b.rs").unwrap();
        let remaining: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM data_flow_edges", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 1);

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
