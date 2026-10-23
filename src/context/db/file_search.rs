use anyhow::Result;
use rusqlite::params;
use super::CodeGraphDb;

#[derive(Debug, Clone)]
pub struct FileMatchResult {
    pub path: String,
    pub top_symbols: Vec<String>,
}

impl CodeGraphDb {
    pub fn search_files(&self, query: &str, limit: usize) -> Result<Vec<FileMatchResult>> {
        let clean = query.trim().trim_start_matches("./").replace('\\', "/");
        if clean.is_empty() {
            return Ok(Vec::new());
        }

        let mut found_paths = Vec::new();

        // 1. Exact path match
        let mut stmt = self.conn.prepare("SELECT path FROM files WHERE path = ? LIMIT 1")?;
        let exact_rows = stmt.query_map(params![clean], |r| r.get::<_, String>(0))?;
        for p in exact_rows.flatten() {
            found_paths.push(p);
        }

        // 2. Basename match (e.g. %/filename.ext or %/filename)
        if found_paths.len() < limit {
            let basename_pattern = format!("%/{}", clean);
            let mut stmt = self.conn.prepare(
                "SELECT path FROM files WHERE path LIKE ? AND path != ? ORDER BY length(path) ASC LIMIT ?"
            )?;
            let rows = stmt.query_map(params![basename_pattern, clean, (limit - found_paths.len()) as i64], |r| {
                r.get::<_, String>(0)
            })?;
            for p in rows.flatten() {
                if !found_paths.contains(&p) {
                    found_paths.push(p);
                }
            }
        }

        // 3. Substring match if still room and query has >= 3 chars
        if found_paths.len() < limit && clean.len() >= 3 {
            let sub_pattern = format!("%{}%", clean);
            let mut stmt = self.conn.prepare(
                "SELECT path FROM files WHERE path LIKE ? ORDER BY length(path) ASC LIMIT ?"
            )?;
            let rows = stmt.query_map(params![sub_pattern, (limit - found_paths.len()) as i64], |r| {
                r.get::<_, String>(0)
            })?;
            for p in rows.flatten() {
                if !found_paths.contains(&p) {
                    found_paths.push(p);
                }
            }
        }

        // Fetch top 3-5 symbols for each matched file
        let mut results = Vec::with_capacity(found_paths.len());
        let mut sym_stmt = self.conn.prepare(
            "SELECT name, kind, start_line FROM symbols WHERE file_path = ? ORDER BY start_line ASC LIMIT 5"
        )?;

        for p in found_paths {
            let sym_rows = sym_stmt.query_map(params![p], |r| {
                let name: String = r.get(0)?;
                let kind: String = r.get(1)?;
                let line: i64 = r.get(2)?;
                Ok(format!("{}:{} ({})", name, line, kind))
            })?;
            let top_symbols = sym_rows.flatten().collect();
            results.push(FileMatchResult {
                path: p,
                top_symbols,
            });
        }

        Ok(results)
    }
}
