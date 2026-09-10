//! 边界感知 + 大小写/空白折叠的字面匹配。favorites(标注反查)与 mentions
//! (证据挂载)共用同一套规则,避免两处各自演化。

use crate::db::graph::normalize_entity_name;

/// 归一化后做边界感知匹配。纯 ASCII needle 要求命中处两侧不是 ASCII 字母数字
/// (否则 "ai" 会命中 "said"/"email");CJK 邻居不是 ASCII 字母数字,所以
/// "GNN模型"/"用AI中台" 正常命中。含 CJK 的 needle 直接 contains(中文无词边界)。
///
/// needle 必须已用 `normalize_entity_name` 归一化;hay 在内部归一化。
pub(crate) fn needle_hit(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hay = normalize_entity_name(hay);
    if hay.is_empty() {
        return false;
    }
    if !needle.is_ascii() {
        return hay.contains(needle);
    }
    hay.match_indices(needle).any(|(start, _)| {
        let end = start + needle.len();
        let before_ok = hay[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric());
        // 右侧:CJK/空格/标点都算边界;英文复数 -s(GNNs/CNNs)也算,
        // 但 "aids" 这类词内命中仍被拦下。
        let rest = &hay[end..];
        let after_ok = match rest.chars().next() {
            None => true,
            Some('s') => rest[1..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_alphanumeric()),
            Some(ch) => !ch.is_ascii_alphanumeric(),
        };
        before_ok && after_ok
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_and_whitespace() {
        assert!(needle_hit("使用 Ai 模型", "ai"));
        assert!(needle_hit("halogen  lithium  exchange", "halogen lithium"));
        assert!(needle_hit("GNN 模型", "gnn"));
    }

    #[test]
    fn ascii_needle_respects_word_boundaries() {
        assert!(!needle_hit("retain", "ai"));
        assert!(!needle_hit("the said email", "ai"));
        assert!(needle_hit("ai 中台", "ai"));
    }

    #[test]
    fn cjk_neighbor_counts_as_boundary() {
        assert!(needle_hit("用AI中台", "ai"));
        assert!(needle_hit("GNN模型很有效", "gnn"));
    }

    #[test]
    fn plural_s_is_allowed_but_not_word_interior() {
        assert!(needle_hit("GNNs are great", "gnn"));
        assert!(!needle_hit("gnnsx", "gnn"));
    }

    #[test]
    fn non_ascii_needle_matches_directly() {
        assert!(needle_hit("注意力机制", "注意力"));
        // 中文无词边界:子串即命中("注意" 是 "注意力机制" 的子串)
        assert!(needle_hit("注意力机制", "注意"));
        assert!(!needle_hit("无关文本", "注意力"));
        assert!(!needle_hit("", "注意力"));
        assert!(!needle_hit("任意文本", ""));
    }
}
