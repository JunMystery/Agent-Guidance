use std::collections::{BTreeMap, HashMap, HashSet};
use super::community::{CommunityHierarchy, CommunityLevel, GraphEdge};

fn is_mermaid_reserved_keyword(id: &str) -> bool {
    matches!(
        id,
        "end"
            | "subgraph"
            | "graph"
            | "flowchart"
            | "style"
            | "classDef"
            | "class"
            | "click"
            | "default"
            | "direction"
            | "linkStyle"
    )
}

pub fn sanitize_mermaid_id(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if sanitized.is_empty()
        || sanitized.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
        || is_mermaid_reserved_keyword(&sanitized)
    {
        format!("node_{}", sanitized)
    } else {
        sanitized
    }
}

pub fn sanitize_mermaid_label(raw: &str) -> String {
    raw.replace('"', "'")
        .replace(
            ['[', ']', '(', ')', '{', '}', '<', '>', '\r', '\n', '\t', ';', '`', '|'],
            " ",
        )
        .trim()
        .to_string()
}

pub fn generate_architecture_mermaid(
    hierarchy: &CommunityHierarchy,
    edges: &[GraphEdge],
) -> String {
    let mut out = String::from("graph TD\n");

    let macro_comms = hierarchy.get_by_level(CommunityLevel::MacroSubsystem);
    if macro_comms.is_empty() {
        out.push_str("    subgraph Architecture [\"Detected Architecture\"]\n");
        out.push_str(&format!(
            "        Arch[\"{}\"]\n",
            sanitize_mermaid_label(&hierarchy.detected_architecture)
        ));
        out.push_str("    end\n");
        return out;
    }

    let mut layer_map: BTreeMap<String, Vec<&super::community::Community>> = BTreeMap::new();
    let mut subgraph_ids: HashSet<String> = HashSet::new();

    for comm in &macro_comms {
        let layer = if comm.summary.layer.trim().is_empty() {
            "Shared".to_string()
        } else {
            comm.summary.layer.clone()
        };
        subgraph_ids.insert(sanitize_mermaid_id(&layer));
        layer_map.entry(layer).or_default().push(*comm);
    }

    let mut entity_to_comm: HashMap<&str, String> = HashMap::new();
    let mut comm_node_ids: HashMap<&str, String> = HashMap::new();

    for comm in &macro_comms {
        let base_id = sanitize_mermaid_id(&comm.id);
        let safe_comm_id = if subgraph_ids.contains(&base_id) {
            format!("comm_{}", base_id)
        } else {
            base_id
        };
        comm_node_ids.insert(comm.id.as_str(), safe_comm_id.clone());
        for member in &comm.member_entity_ids {
            entity_to_comm.insert(member.as_str(), safe_comm_id.clone());
        }
    }

    // Render Subgraphs by Layer
    for (layer_name, comms) in layer_map {
        let safe_subgraph_id = sanitize_mermaid_id(&layer_name);
        out.push_str(&format!(
            "    subgraph {} [\"{} Layer\"]\n",
            safe_subgraph_id,
            sanitize_mermaid_label(&layer_name)
        ));
        for comm in comms {
            let safe_id = comm_node_ids
                .get(comm.id.as_str())
                .cloned()
                .unwrap_or_else(|| sanitize_mermaid_id(&comm.id));
            let safe_title = sanitize_mermaid_label(&comm.summary.title);
            out.push_str(&format!("        {}[\"{}\"]\n", safe_id, safe_title));
        }
        out.push_str("    end\n");
    }

    // Connect Inter-Community Edges
    let mut rendered_edges: HashSet<(String, String)> = HashSet::new();
    for edge in edges {
        if let (Some(src_comm), Some(tgt_comm)) = (
            entity_to_comm.get(edge.source_id.as_str()),
            entity_to_comm.get(edge.target_id.as_str()),
        ) {
            if src_comm != tgt_comm && !rendered_edges.contains(&(src_comm.clone(), tgt_comm.clone())) {
                out.push_str(&format!("    {} --> {}\n", src_comm, tgt_comm));
                rendered_edges.insert((src_comm.clone(), tgt_comm.clone()));
            }
        }
    }

    out
}

pub fn generate_blast_radius_mermaid(
    target_symbol: &str,
    callers: &[String],
    callees: &[String],
) -> String {
    let mut out = String::from("graph LR\n");
    let safe_target = sanitize_mermaid_id(target_symbol);
    let target_label = sanitize_mermaid_label(target_symbol);

    out.push_str(&format!("    {}[\"{}\"]\n", safe_target, target_label));

    for caller in callers {
        let s_id = sanitize_mermaid_id(caller);
        let s_label = sanitize_mermaid_label(caller);
        out.push_str(&format!("    {}[\"{}\"] --> {}\n", s_id, s_label, safe_target));
        out.push_str(&format!("    style {} fill:#2b3a4a,stroke:#4a90e2,stroke-width:1px\n", s_id));
    }

    for callee in callees {
        let s_id = sanitize_mermaid_id(callee);
        let s_label = sanitize_mermaid_label(callee);
        out.push_str(&format!("    {} --> {}[\"{}\"]\n", safe_target, s_id, s_label));
        out.push_str(&format!("    style {} fill:#2b4a3a,stroke:#50e3c2,stroke-width:1px\n", s_id));
    }

    out.push_str(&format!(
        "    style {} fill:#4a3b2b,stroke:#f5a623,stroke-width:2px\n",
        safe_target
    ));

    out
}

#[cfg(test)]
#[path = "mermaid_tests.rs"]
mod tests;

#[cfg(test)]
mod regression_tests {
    use super::super::community::{Community, CommunityHierarchy, CommunityLevel, CommunitySummary};
    use super::*;

    #[test]
    fn test_reserved_keyword_and_injection_sanitization() {
        assert_eq!(sanitize_mermaid_id("end"), "node_end");
        assert_eq!(sanitize_mermaid_id("subgraph"), "node_subgraph");
        assert_eq!(sanitize_mermaid_id("style"), "node_style");
        assert_eq!(sanitize_mermaid_id("click"), "node_click");
        assert!(!sanitize_mermaid_label("foo\nclick A \"javascript:alert(1)\";`bar`").contains('\n'));
        assert!(!sanitize_mermaid_label("a\r\nb;c|d`e").contains(['\r', '\n', ';', '|', '`']));
    }

    #[test]
    fn test_architecture_mermaid_avoids_subgraph_node_id_collision() {
        let mut hierarchy = CommunityHierarchy::new("proj", "Layered");
        hierarchy.communities.push(Community {
            id: "Shared".to_string(),
            level: CommunityLevel::MacroSubsystem,
            parent_id: None,
            member_entity_ids: vec!["sym_a".to_string()],
            member_files: vec!["src/shared.rs".to_string()],
            summary: CommunitySummary {
                title: "Shared".to_string(),
                description: "Shared utilities".to_string(),
                layer: "Shared".to_string(),
                key_entities: vec!["sym_a".to_string()],
                export_interfaces: vec![],
                dependencies: vec![],
            },
        });

        let mermaid = generate_architecture_mermaid(&hierarchy, &[]);
        assert!(mermaid.contains("subgraph Shared [\"Shared Layer\"]"));
        assert!(mermaid.contains("comm_Shared[\"Shared\"]"));
        assert!(!mermaid.contains("        Shared[\"Shared\"]"));
    }
}

