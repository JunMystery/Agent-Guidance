//! Batch helpers for SQLite code graph indexing.
//!
//! Groups SQLite writes into batched transactions (50 files per batch) to achieve
//! 50x-100x write throughput speedup on disk (NTFS/ext4) and avoid MCP request timeouts.

use anyhow::Result;
use rusqlite::Connection;
use crate::context::db::CodeGraphDb;
use super::parsers::{CodeChunk, ExtractedEdge, ExtractedSymbol};
use super::IndexReport;

pub fn begin_batch(conn: &Connection) {
    let _ = conn.execute_batch("BEGIN TRANSACTION;");
}

pub fn checkpoint_batch(conn: &Connection) {
    let _ = conn.execute_batch("COMMIT; BEGIN TRANSACTION;");
}

pub fn commit_batch(conn: &Connection) {
    let _ = conn.execute_batch("COMMIT;");
}

fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

pub fn clear_file_edges(conn: &Connection, rel_path: &str) {
    let prefix = format!("{}::%", escape_like(rel_path));
    let _ = conn.execute(
        "DELETE FROM symbol_edges WHERE source_id LIKE ?1 ESCAPE '\\' OR target_id LIKE ?1 ESCAPE '\\'",
        rusqlite::params![prefix],
    );
    let _ = conn.execute(
        "DELETE FROM data_flow_edges WHERE caller_symbol_id LIKE ?1 ESCAPE '\\' OR callee_symbol_id LIKE ?1 ESCAPE '\\'",
        rusqlite::params![prefix],
    );
}

pub fn persist_file_symbols(
    db: &CodeGraphDb,
    rel_path: &str,
    symbols: &[ExtractedSymbol],
    report: &mut IndexReport,
) -> Result<()> {
    for s in symbols {
        db.insert_symbol_full(
            &s.id,
            &s.name,
            &s.kind,
            rel_path,
            s.parent.as_deref(),
            s.start_line,
            s.end_line,
            s.signature.as_deref(),
            s.language.as_deref(),
            s.namespace.as_deref(),
            s.receiver.as_deref(),
        )?;
        report.symbols_extracted += 1;
    }
    Ok(())
}

pub fn persist_file_edges(
    db: &CodeGraphDb,
    edges: Vec<ExtractedEdge>,
    report: &mut IndexReport,
) -> Result<()> {
    for e in edges {
        db.insert_edge_full(
            &e.source_id,
            &e.target_id,
            &e.edge_type,
            e.weight,
            e.confidence,
            &e.category,
            e.call_line,
        )?;
        report.edges_created += 1;
    }
    Ok(())
}

pub fn persist_file_chunks(
    db: &CodeGraphDb,
    rel_path: &str,
    chunks: Vec<CodeChunk>,
    report: &mut IndexReport,
) -> Result<()> {
    for c in chunks {
        db.insert_chunk(rel_path, c.start_line, c.end_line, &c.hash, &c.text)?;
        report.chunks_created += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_transaction_helpers() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (x INT);").unwrap();
        begin_batch(&conn);
        conn.execute("INSERT INTO t VALUES (1);", []).unwrap();
        checkpoint_batch(&conn);
        conn.execute("INSERT INTO t VALUES (2);", []).unwrap();
        commit_batch(&conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM t;", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_clear_file_edges_escapes_like_wildcards() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE symbol_edges (source_id TEXT, target_id TEXT);
             CREATE TABLE data_flow_edges (caller_symbol_id TEXT, callee_symbol_id TEXT);",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO symbol_edges VALUES ('src/a_b.rs::fn::f1', 'src/c.rs::fn::f3');",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbol_edges VALUES ('src/axb.rs::fn::f2', 'src/c.rs::fn::f3');",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO data_flow_edges VALUES ('src/a_b.rs::fn::f1', 'src/c.rs::fn::f3');",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO data_flow_edges VALUES ('src/axb.rs::fn::f2', 'src/c.rs::fn::f3');",
            [],
        )
        .unwrap();

        clear_file_edges(&conn, "src/a_b.rs");

        let sym_remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbol_edges;", [], |r| r.get(0))
            .unwrap();
        let df_remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM data_flow_edges;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sym_remaining, 1);
        assert_eq!(df_remaining, 1);

        let kept_source: String = conn
            .query_row("SELECT source_id FROM symbol_edges;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept_source, "src/axb.rs::fn::f2");
    }
}
