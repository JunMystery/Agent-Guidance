# Auto Skill Discovery Pipeline

This document describes the three-tier pipeline used by the Agent Guidance MCP server to automatically detect, classify, and match skills/guidelines for user tasks.

---

## Architecture Workflow

```mermaid
graph TD
    A["User Task Prompt (e.g., 'Make an Android app')"] --> B["Tokenization (lowercase, split)"]
    B --> C["Keyword Matching & Set Intersection"]
    D["Frontmatter 'triggers' in SKILL.md"] --> C
    E["Built-in TASK_KEYWORD_TRIGGERS"] --> C
    C --> F["Matches (Keywords & Identifiers)"]
    F --> G["Hybrid Vector Search (embedding + keyword boost)"]
    F --> H["O(1) Task Anchor Retrieval"]
    G --> I["Cross-Encoder Rerank (minilm)"]
    H --> I
    I --> J["Final Recommendations List"]
```

---

## Processing Phases

### 1. Tokenization & Normalization
* **Files**: `src/mcp/tools/pipeline.rs` & `src/ml/embeddings/search.rs`
* **Action**: Extracts tokens from the task string, filters out single-character words, converts to lowercase. Embedded in the hybrid search pipeline.

### 2. Flexible Keyword Inference & Root Mapping
* **Files**: `src/mcp/tools/guidance_intent.rs` & `src/ml/embeddings/search.rs`
* **Action**: Compares extracted task terms against skill names and content. Three matching levels:
  * **Exact Match**: skill name equals query → +0.5 score boost
  * **Name Contains**: skill name contains query → +0.3 score boost
  * **Word Match**: query word found in skill name → +0.1 score boost each

### 3. Catalog Matching & Anchor Routing
* **Files**: `src/catalog/store.rs` (`list_embedded_skills`) & `src/mcp/tools/pipeline.rs` (`task_pipeline`)
* **Action**: Loads all 629 embedded skills + staged/workspace-local skills. Matches keywords to pull relevant entries:
  * **Anchor Promotion**: Frontmatter `anchors` promote matching skills to top recommendations.
  * **Name/Content Keyword Scoring**: Additional weight for skills matching task keywords.

### 4. Hybrid Vector Search & Cross-Encoder Rerank
* **Files**: `src/ml/embeddings/search.rs` (`hybrid_vector_search`), `src/ml/cross_encoder.rs` (`CrossEncoder`), & `src/ml/llm_selector.rs` (`LLMSelector::rerank`)
* **Action**: Generates a query vector embedding via multilingual-e5-small, scores all 629 precomputed skill passage vectors by cosine similarity, then re-ranks top-8 with a cross-encoder:
  * **Cached Passage Vectors**: All skills pre-embedded during `warmup_cache()` at daemon startup — stored in `PASSAGE_CACHE` module-level `OnceLock` and persisted locally at `~/.agent-guidance/passage_vectors.bin` (separate from the remote ML worker's compiled `vectors.bin`)
  * **Hybrid Score**: `cosine_similarity(q_vec, passage_vec) + keyword_boost`
  * **Cross-Encoder Rerank**: `LLMSelector::rerank()` scores top-8 with `cross-encoder/ms-marco-MiniLM-L-6-v2` (single-output regression, `num_labels=1`) using native sentence-pair encoding `(query, candidate)` with `token_type_ids` and 512-token truncation
  * **Graceful Fallback**: If the cross-encoder fails, falls back to keyword-frequency ranking

---

## Extending Triggers in Skill Files

To register any new or existing skill in the Auto Skill Discovery pipeline, simply add frontmatter tags to the skill's `SKILL.md` file:

```yaml
---
name: my-custom-skill
description: Guidelines for custom operations
triggers: [custom, operations, modularize, modular]
anchors: [custom-anchor]
dependencies: [base-standard-identifier]
---
```
* Adding words to `triggers` will automatically feed them to the matching engine.
* Declaring `anchors` will map those keywords to this file as top-priority recommendations.
