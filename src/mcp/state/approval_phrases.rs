use super::ServerState;

fn has_explicit_negation(msg: &str) -> bool {
    let negation_phrases = [
        "từ chối",
        "chưa đồng ý",
        "không đồng ý",
        "không duyệt",
        "chưa duyệt",
        "do not",
        "does not",
        "did not",
        "will not",
        "would not",
        "not ok",
        "not approved",
        "not yet",
        "hold on",
    ];
    if negation_phrases
        .iter()
        .any(|phrase| contains_phrase_boundary(msg, phrase))
    {
        return true;
    }

    msg.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|s| !s.is_empty())
        .any(|tok| {
            matches!(
                tok,
                "no" | "not"
                    | "don't"
                    | "dont"
                    | "won't"
                    | "wont"
                    | "can't"
                    | "cant"
                    | "cannot"
                    | "shouldn't"
                    | "wouldn't"
                    | "reject"
                    | "rejected"
                    | "cancel"
                    | "canceled"
                    | "cancelled"
                    | "never"
                    | "wait"
                    | "stop"
                    | "abort"
                    | "decline"
                    | "declined"
                    | "deny"
                    | "denied"
                    | "không"
                    | "chưa"
                    | "đừng"
                    | "khoan"
                    | "hủy"
                    | "huỷ"
            )
        })
}

impl ServerState {
    pub fn is_negated_message(message: &str) -> bool {
        let msg = message.trim().to_lowercase();
        !msg.is_empty() && has_explicit_negation(&msg)
    }

    pub fn is_approval_message(message: &str) -> bool {
        let msg = message.trim().to_lowercase();
        if msg.is_empty() {
            return false;
        }

        if has_explicit_negation(&msg) {
            return false;
        }

        let phrase_keywords = [
            "the user has approved this document",
            "proceeded with implementation plan",
            "proceed with implementation plan",
            "approve plan",
            "proceed with",
            "go ahead",
            "do it",
            "làm đi",
            "đồng ý",
            "chấp nhận",
            "looks good",
            "let's do it",
            "make the change",
            "triển khai",
            "tiến hành",
            "ok bro",
            "được đấy",
        ];

        for phrase in phrase_keywords {
            if contains_phrase_boundary(&msg, phrase) {
                return true;
            }
        }

        let word_keywords = [
            "ok", "proceed", "approved", "approve", "yes", "yep",
            "lgtm", "agree", "duyệt", "thực thi", "chốt",
        ];

        for kw in word_keywords {
            if kw.contains(' ') {
                if contains_phrase_boundary(&msg, kw) {
                    return true;
                }
            } else if msg
                .split(|c: char| !c.is_alphanumeric())
                .any(|tok| tok == kw)
            {
                return true;
            }
        }

        false
    }

    pub fn is_gui_or_short_plan_approval(message: &str) -> bool {
        let msg = message.trim().to_lowercase();
        if msg.is_empty() {
            return false;
        }
        if has_explicit_negation(&msg) {
            return false;
        }
        let gui_phrases = [
            "the user has approved this document",
            "proceeded with implementation plan",
            "proceed with implementation plan",
            "approve plan",
        ];
        if gui_phrases.iter().any(|p| msg.contains(p)) {
            return true;
        }
        if msg.split_whitespace().count() > 6 {
            return false;
        }

        let short_phrases = [
            "go ahead", "looks good", "làm đi", "đồng ý", "chấp nhận",
            "ok bro", "được đấy", "triển khai đi", "tiến hành đi",
            "let's do it", "make the change",
        ];
        if short_phrases.iter().any(|p| contains_phrase_boundary(&msg, p)) {
            return true;
        }

        let short_words = ["ok", "proceed", "approved", "lgtm", "yes", "yep", "agree", "duyệt", "chốt"];
        msg.split(|c: char| !c.is_alphanumeric())
            .any(|tok| short_words.contains(&tok))
    }

    pub fn process_user_message(&mut self, message: &str) -> bool {
        if Self::is_approval_message(message) {
            self.plan_approved = true;
            return true;
        }
        false
    }
}

fn contains_phrase_boundary(haystack: &str, phrase: &str) -> bool {
    if phrase.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(idx) = haystack[start..].find(phrase) {
        let abs = start + idx;
        let before_ok = haystack[..abs]
            .chars()
            .next_back()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        let after_idx = abs + phrase.len();
        let after_ok = haystack[after_idx..]
            .chars()
            .next()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        start = abs + phrase.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_negation_with_punctuation_blocks_approval() {
        assert!(!ServerState::is_approval_message("No, don't proceed"));
        assert!(!ServerState::is_approval_message("No! not ok"));
        assert!(!ServerState::is_approval_message("Không, chưa đồng ý"));
        assert!(!ServerState::is_gui_or_short_plan_approval("No, don't proceed"));
        assert!(ServerState::is_approval_message("OK, proceed"));
        assert!(ServerState::is_gui_or_short_plan_approval("Yes, go ahead"));
    }

    #[test]
    fn test_contains_phrase_boundary_empty_phrase_no_panic_on_utf8() {
        assert!(!contains_phrase_boundary("đồng ý triển khai", ""));
    }
}
