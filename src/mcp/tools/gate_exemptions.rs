use std::path::Path;

/// Determines if a file is exempt from the 300 LOC limit (documentation, configs, datasets, assets, etc.).
/// All active application and test source code files MUST remain strictly non-exempt (< 300 LOC).
pub fn is_exempt_from_loc_limit(rel_path: &str) -> bool {
    let clean = rel_path.trim().replace('\\', "/");
    let path = Path::new(&clean);
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let name_upper = file_name.to_uppercase();

    // 1. Exact or prefix matches for special filenames and dotfiles
    if file_name.starts_with(".env")
        || file_name.starts_with(".prettier")
        || file_name.starts_with(".eslint")
        || matches!(
            name_upper.as_str(),
            ".GITIGNORE"
                | ".GITATTRIBUTES"
                | ".GITMODULES"
                | ".GITKEEP"
                | ".DOCKERIGNORE"
                | ".EDITORCONFIG"
                | ".NPMRC"
                | "LICENSE"
                | "LICENCE"
                | "README"
                | "CHANGELOG"
                | "AUTHORS"
                | "CONTRIBUTING"
                | "MAKEFILE"
                | "DOCKERFILE"
                | "CONTAINERFILE"
                | "PROCFILE"
                | "JENKINSFILE"
        )
    {
        return true;
    }

    // 2. Extension matching for non-code files
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        matches!(
            ext_lower.as_str(),
            // Documentation & Markup
            "md" | "markdown" | "mdown" | "mdx" | "txt" | "rst" | "adoc" | "doc" | "docx" | "pdf"
            // Data, Serialization & Configurations
            | "json" | "json5" | "jsonl" | "csv" | "tsv" | "yaml" | "yml" | "xml" | "toml" | "lock"
            | "sql" | "env" | "ini" | "cfg" | "conf" | "sum" | "properties" | "parquet" | "dataset"
            | "arrow" | "proto" | "graphql" | "gql" | "plist" | "xcconfig" | "sample"
            // Static Assets & Web Styling
            | "html" | "htm" | "css" | "scss" | "sass" | "less" | "svg" | "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp"
            // Logs, VCS & Patches
            | "log" | "diff" | "patch"
        )
    } else {
        false
    }
}
