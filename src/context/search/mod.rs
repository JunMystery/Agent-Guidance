//! Search intelligence engine: Intent Gating, Dynamic IDF down-weighting, and Role Penalties.

pub mod idf;
pub mod intent;
pub mod scorer;
pub mod token_expand;

pub use idf::{
    analyze_query_terms, compute_query_downweight_factor, TermDocFreq,
};
pub use intent::{
    classify_intent, SearchIntent,
};
pub use scorer::{calculate_hit_score, sort_and_dedup_results, RankedResult};
pub use token_expand::{analyze_query_tokens, strip_conversational_words};

use crate::context::db::CodeGraphDb;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SearchExecutionResult {
    pub results: Vec<RankedResult>,
    pub intent: SearchIntent,
    pub source: &'static str,
    pub term_weights: Vec<TermDocFreq>,
    pub ubiquitous_factor: f64,
}

pub fn execute_ranked_search<F>(
    db: &CodeGraphDb,
    query: &str,
    intent_override: Option<&str>,
    limit: usize,
    embed_fn: F,
) -> SearchExecutionResult
where
    F: Fn(&str) -> Option<Vec<f32>>,
{
    let intent = match intent_override {
        Some(opt) if !opt.is_empty() && opt != "auto" => SearchIntent::from_str_opt(Some(opt)),
        _ => classify_intent(query),
    };

    let term_weights = analyze_query_terms(&db.conn, query);
    let ubiquitous_factor = compute_query_downweight_factor(&term_weights);
    let mut candidate_hits = Vec::new();
    let mut primary_source = "symbol_fts";

    // 0. File Path Search (Layer 1: Exact / Basename / Substring path matches)
    if let Ok(files) = db.search_files(query, 3) {
        if !files.is_empty() {
            primary_source = "file_match";
            for f in files {
                let score = calculate_hit_score(&f.path, 2.50, query, intent, ubiquitous_factor);
                let snippet = (!f.top_symbols.is_empty()).then(|| format!("outline: [{}]", f.top_symbols.join(", ")));
                candidate_hits.push(RankedResult {
                    path: f.path.clone(),
                    name: f.path,
                    line: 1,
                    end_line: None,
                    score,
                    source: "file_match",
                    snippet,
                });
            }
        }
    }

    // 1. Alias cache lookup (Instant, highest confidence for non-code queries)
    let is_code_query = query.chars().any(|c| {
        matches!(c, '@' | '(' | ')' | '{' | '}' | '[' | ']' | ';' | ':' | '<' | '>' | '=' | '/' | '\\' | '"' | '\'' | '`' | '.')
    }) || query.contains("::") || query.contains("->");

    if !is_code_query {
        if let Ok(aliases) = db.lookup_aliases(query, limit) {
            if !aliases.is_empty() {
                let hits: Vec<RankedResult> = aliases
                    .into_iter()
                    .map(|a| {
                        let score = calculate_hit_score(
                            &a.resolved_path,
                            a.confidence * 2.0,
                            query,
                            intent,
                            ubiquitous_factor,
                        );
                        RankedResult {
                            path: a.resolved_path,
                            name: a.resolved_symbol.unwrap_or_else(|| "—".to_string()),
                            line: a.resolved_line.unwrap_or(1),
                            end_line: None,
                            score,
                            source: "alias_cache",
                            snippet: None,
                        }
                    })
                    .collect();

                let sorted = sort_and_dedup_results(hits, limit);
                if sorted.len() >= limit && intent != SearchIntent::Logic {
                    return SearchExecutionResult {
                        results: sorted,
                        intent,
                        source: "alias_cache",
                        term_weights,
                        ubiquitous_factor,
                    };
                } else if !sorted.is_empty() {
                    primary_source = "alias_cache";
                    candidate_hits.extend(sorted);
                }
            }
        }
    }

    // 2. Symbols FTS with Token Expansion (Layer 2: BM25 ranking + Qualifier + Case variants)
    let expansion = analyze_query_tokens(query);
    let mut search_tokens = vec![query.to_string()];
    if !expansion.symbol_name.is_empty() && expansion.symbol_name != query {
        search_tokens.push(expansion.symbol_name.clone());
    }
    for variant in &expansion.case_variants {
        if !search_tokens.contains(variant) {
            search_tokens.push(variant.clone());
        }
    }

    let mut sym_hits = Vec::new();
    for token in &search_tokens {
        if let Ok(syms) = db.search_symbols(token, limit * 2) {
            if !syms.is_empty() {
                sym_hits = syms;
                break;
            }
        }
    }

    if sym_hits.is_empty() {
        let stripped = strip_conversational_words(query);
        if !stripped.is_empty() && stripped != query {
            if let Ok(syms) = db.search_symbols(&stripped, limit * 2) {
                sym_hits = syms;
            }
        }
    }

    if !sym_hits.is_empty() {
        if candidate_hits.is_empty() {
            primary_source = "symbol_fts";
        }
        for (idx, (path, name, line)) in sym_hits.into_iter().enumerate() {
            let mut raw_score = 1.0 / (1.0 + idx as f64 * 0.1);
            if let Some(ref q) = expansion.qualifier {
                if path.contains(q) || name.starts_with(q) {
                    raw_score *= 1.35;
                }
            }
            let score = calculate_hit_score(&path, raw_score, query, intent, ubiquitous_factor);
            candidate_hits.push(RankedResult {
                path,
                name,
                line,
                end_line: None,
                score,
                source: "symbol_fts",
                snippet: None,
            });
        }
    }

    // 3. Content FTS5 with BM25 ranking (Fast lexical fallback before neural embedding)
    let has_symbol_hits = candidate_hits.iter().any(|r| r.source == "symbol_fts");
    if candidate_hits.is_empty() || (!has_symbol_hits && candidate_hits.len() < limit) {
        let content_res = db.search_content_fts(query, limit * 2)
            .or_else(|_| Ok(Vec::new()))
            .and_then(|res| {
                if res.is_empty() {
                    let stripped = strip_conversational_words(query);
                    if !stripped.is_empty() && stripped != query {
                        return db.search_content_fts(&stripped, limit * 2);
                    }
                }
                Ok(res)
            });

        if let Ok(content_hits) = content_res {
            if !content_hits.is_empty() {
                if candidate_hits.is_empty() {
                    primary_source = "content_fts";
                }
                for (idx, (path, start, end, snip)) in content_hits.into_iter().enumerate() {
                    let raw_score = 0.9 / (1.0 + idx as f64 * 0.1);
                    let score = calculate_hit_score(&path, raw_score, query, intent, ubiquitous_factor);
                    candidate_hits.push(RankedResult {
                        path,
                        name: format!("L{}-{}", start, end),
                        line: start,
                        end_line: Some(end),
                        score,
                        source: "content_fts",
                        snippet: Some(snip),
                    });
                }
            }
        }
    }

    // 4. Hybrid Neural Vector Search (Layer 3: Activated if candidate hits low or score weak)
    let max_score = candidate_hits.iter().map(|r| r.score).fold(0.0, f64::max);
    let should_vector_search = candidate_hits.len() < limit || max_score < 0.85;

    if should_vector_search {
        if let Some(qv) = embed_fn(query) {
            if let Ok(vec_syms) = db.vector_search_symbols(&qv, limit * 2, 0.60) {
                if !vec_syms.is_empty() {
                    if candidate_hits.is_empty() {
                        primary_source = "symbol_vector";
                    }
                    for v in vec_syms {
                        let score = calculate_hit_score(
                            &v.file_path,
                            v.score as f64,
                            query,
                            intent,
                            ubiquitous_factor,
                        );
                        let snippet = v.signature.as_ref().map(|s| {
                            if let Some(ref p) = v.parent {
                                format!("{} in {}", s, p)
                            } else {
                                s.clone()
                            }
                        });
                        candidate_hits.push(RankedResult {
                            path: v.file_path,
                            name: v.name,
                            line: v.start_line,
                            end_line: v.end_line,
                            score,
                            source: "symbol_vector",
                            snippet,
                        });
                    }
                }
            }

            if candidate_hits.len() < limit {
                if let Ok(chunks) = db.vector_search_chunks(&qv, limit * 2, 0.55) {
                    if !chunks.is_empty() {
                        if candidate_hits.is_empty() {
                            primary_source = "content_vector";
                        }
                        for c in chunks {
                            let score = calculate_hit_score(
                                &c.file_path,
                                c.score as f64,
                                query,
                                intent,
                                ubiquitous_factor,
                            );
                            let snip_preview: String = c.chunk_text.chars().take(120).collect();
                            candidate_hits.push(RankedResult {
                                path: c.file_path,
                                name: format!("L{}-{}", c.start_line, c.end_line),
                                line: c.start_line,
                                end_line: Some(c.end_line),
                                score,
                                source: "content_vector",
                                snippet: if snip_preview.is_empty() { None } else { Some(snip_preview) },
                            });
                        }
                    }
                }
            }
        }
    }

    let sorted = sort_and_dedup_results(candidate_hits, limit);

    SearchExecutionResult {
        results: sorted,
        intent,
        source: primary_source,
        term_weights,
        ubiquitous_factor,
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
