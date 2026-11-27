use std::path::Path;

/// Checks whether a new file name or justification suggests cross-domain bundling,
/// returning a soft domain/feature cohesion advisory instead of a hard block.
pub(crate) fn validate_new_file_modularity(rel_path: &str, justification: &str) -> Option<String> {
    if crate::context::ast::metrics::is_test_file_path(rel_path) {
        return None;
    }

    let file_path = Path::new(rel_path);
    let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let file_stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let clean_stem = file_stem.replace('-', "_");

    // 1. Language-agnostic plural container suffixes (e.g. *utils, *helpers, *services)
    let compound_suffixes = [
        "modals", "dialogs", "drawers", "forms", "tables", "cards", "panels", "widgets",
        "components", "views", "screens", "pages",
        "services", "handlers", "controllers", "managers", "repositories", "endpoints",
        "routes", "actions", "mutations", "queries", "reducers",
        "helpers", "utils", "utilities", "models", "entities", "adapters", "transformers", "listeners",
        "providers", "subscribers", "factories", "builders", "validators", "converters",
        "processors", "resolvers", "types",
    ];

    for suffix in compound_suffixes {
        if clean_stem == suffix
            || clean_stem.ends_with(&format!("_{}", suffix))
            || (clean_stem.len() > suffix.len() && clean_stem.ends_with(suffix))
        {
            return Some(format!(
                "**DOMAIN_COHESION_ADVISORY**: New file `{}` (`{}`) uses container suffix `*{}`. Verify it serves ONE cohesive domain/use case (e.g., `orders/` or `auth/`) rather than bundling unrelated utilities across modules.",
                rel_path, file_name, suffix
            ));
        }
    }

    // 2. Multi-component justification check
    let j_lower = justification.to_lowercase();
    let has_conjunction = j_lower.contains(" and ") || j_lower.contains(" & ") || j_lower.contains(" cùng ") || j_lower.contains(" và ") || j_lower.contains(",");
    let has_action = j_lower.contains("create") || j_lower.contains("add") || j_lower.contains("implement") || j_lower.contains("build") || j_lower.contains("introduce");

    if has_conjunction && has_action {
        let words: Vec<&str> = justification.split(|c: char| c.is_whitespace() || c == ',' || c == '&').filter(|s| !s.trim().is_empty()).collect();
        let entity_count = words.iter().filter(|w| {
            let clean = w.trim_matches(|c: char| !c.is_alphanumeric());
            (clean.ends_with("Modal") || clean.ends_with("Dialog") || clean.ends_with("View") || clean.ends_with("Component")
                || clean.ends_with("Service") || clean.ends_with("Handler") || clean.ends_with("Controller")
                || clean.ends_with("Repo") || clean.ends_with("Repository") || clean.ends_with("Screen")
                || clean.ends_with("Manager") || clean.ends_with("Router") || clean.ends_with("Adapter"))
                && clean.len() > 6
        }).count();

        if entity_count >= 2 {
            return Some(format!(
                "**DOMAIN_COHESION_ADVISORY**: Justification for `{}` lists {} distinct components. Ensure they belong to the same feature/domain and keep each function small (15-40 LOC).",
                rel_path, entity_count
            ));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advises_on_plural_compound_names_across_languages() {
        assert!(validate_new_file_modularity("src/components/InventoryModals.tsx", "Modals").unwrap().contains("DOMAIN_COHESION_ADVISORY"));
        assert!(validate_new_file_modularity("src/views/DashboardWidgets.vue", "Widgets").is_some());
        assert!(validate_new_file_modularity("internal/services/order_services.go", "Order services").is_some());
        assert!(validate_new_file_modularity("internal/handlers/auth_handlers.go", "Auth handlers").is_some());
        assert!(validate_new_file_modularity("src/controllers/user_controllers.rs", "User controllers").is_some());
        assert!(validate_new_file_modularity("src/utils/general_helpers.rs", "Helpers").is_some());
        assert!(validate_new_file_modularity("app/api/item_endpoints.py", "Endpoints").is_some());
        assert!(validate_new_file_modularity("app/core/user_validators.py", "Validators").is_some());
        assert!(validate_new_file_modularity("app/common/data_types.py", "Types").is_some());
    }

    #[test]
    fn test_allows_single_responsibility_names() {
        assert!(validate_new_file_modularity("src/components/CreateItemModal.tsx", "Create item modal").is_none());
        assert!(validate_new_file_modularity("internal/service/order_service.go", "Order service").is_none());
        assert!(validate_new_file_modularity("src/mcp/tools/gate_edit.rs", "Edit gate handler").is_none());
        assert!(validate_new_file_modularity("app/api/item_router.py", "Item router").is_none());
    }

    #[test]
    fn test_advises_on_multi_component_justifications() {
        let res = validate_new_file_modularity(
            "src/components/InventoryDialog.tsx",
            "Add CreateItemModal and AdjustStockModal for inventory",
        );
        assert!(res.is_some());
        assert!(res.unwrap().contains("DOMAIN_COHESION_ADVISORY"));
    }

    #[test]
    fn test_advises_on_exact_plural_stem_and_excludes_test_files() {
        let adv = validate_new_file_modularity("src/utils.rs", "Shared helpers");
        assert!(adv.is_some());
        assert!(adv.unwrap().contains("DOMAIN_COHESION_ADVISORY"));

        assert!(validate_new_file_modularity("src/helpers.rs", "Helpers").is_some());
        assert!(validate_new_file_modularity("tests/test_helpers.rs", "Test helpers").is_none());
        assert!(
            validate_new_file_modularity("tests/integration_utils.rs", "Add modal and service helpers")
                .is_none()
        );
    }
}
