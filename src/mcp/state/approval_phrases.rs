use super::ServerState;

impl ServerState {
    pub fn is_approval_message(message: &str) -> bool {
        let msg = message.trim().to_lowercase();
        if msg.is_empty() {
            return false;
        }

        let negation_prefixes = ["no ", "not ", "don't ", "dont ", "reject", "cancel", "không ", "hủy", "từ chối"];
        if msg.split_whitespace().count() <= 10 && negation_prefixes.iter().any(|p| msg.starts_with(p)) {
            return false;
        }

        let phrase_keywords = [
            "the user has approved this document",
            "proceeded with implementation plan",
            "comments on artifact uri",
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
            "ok", "proceed", "approved", "approve", "start", "yes", "yep",
            "lgtm", "agree", "exec", "duyệt", "thực thi", "chốt",
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
        let gui_phrases = [
            "the user has approved this document",
            "proceeded with implementation plan",
            "comments on artifact uri",
            "proceed with implementation plan",
            "approve plan",
        ];
        if gui_phrases.iter().any(|p| msg.contains(p)) {
            return true;
        }
        if msg.split_whitespace().count() > 6 {
            return false;
        }

        let negation_prefixes = ["no ", "not ", "don't ", "dont ", "reject", "cancel", "không ", "hủy", "từ chối"];
        if negation_prefixes.iter().any(|p| msg.starts_with(p)) {
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
