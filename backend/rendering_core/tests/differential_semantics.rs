// Differential (golden replay) tests: semantics classification functions.

mod common;

use common::*;
use rendering_core::semantics::{
    block_kind, is_bodylike_block, is_caption_like_block, is_footnote_like_block,
    is_metadata_semantic, is_plain_text_block, is_textual_block, is_title_like_block, layout_role,
    structure_role,
};

macro_rules! item_str_test {
    ($name:ident, $fn_id:literal, $f:path) => {
        #[test]
        fn $name() {
            let c = corpus();
            for (_i, case) in cases(c, $fn_id).iter().enumerate() {
                let item: ItemDto = from_value(&case.input["item"]);
                let expected: String = from_value(&case.expected);
                let actual = $f(&item.to_item());
                assert_eq!(actual, expected);
            }
        }
    };
}

macro_rules! item_bool_test {
    ($name:ident, $fn_id:literal, $f:path) => {
        #[test]
        fn $name() {
            let c = corpus();
            for (i, case) in cases(c, $fn_id).iter().enumerate() {
                let item: ItemDto = from_value(&case.input["item"]);
                let expected: bool = from_value(&case.expected);
                let actual = $f(&item.to_item());
                assert_eq!(actual, expected, "mismatch at case {i}");
            }
        }
    };
}

item_str_test!(test_layout_role, "semantics.layout_role", layout_role);
item_str_test!(test_block_kind, "semantics.block_kind", block_kind);
item_str_test!(test_structure_role, "semantics.structure_role", structure_role);
item_bool_test!(test_is_caption_like_block, "semantics.is_caption_like_block", is_caption_like_block);
item_bool_test!(test_is_footnote_like_block, "semantics.is_footnote_like_block", is_footnote_like_block);
item_bool_test!(test_is_title_like_block, "semantics.is_title_like_block", is_title_like_block);
item_bool_test!(test_is_bodylike_block, "semantics.is_bodylike_block", is_bodylike_block);
item_bool_test!(test_is_textual_block, "semantics.is_textual_block", is_textual_block);
item_bool_test!(test_is_plain_text_block, "semantics.is_plain_text_block", is_plain_text_block);
item_bool_test!(test_is_metadata_semantic, "semantics.is_metadata_semantic", is_metadata_semantic);
