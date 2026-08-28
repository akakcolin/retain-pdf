"""Bridge boundary shapes for the Python→Rust rendering contract.

Each `_native.py` shim / bundle producer serializes a hand-written JSON payload
that the Rust side deserializes with a lenient serde DTO. This module declares
the authoritative key set for the six highest-value boundaries and provides
`assert_keys_match` — the key-set gate the differential corpus generator calls
after producing, so a drift (missing required key, undeclared extra key) fails
loudly instead of silently deserializing.

`total=False` marks keys the producer genuinely omits. Boundaries whose Rust DTO
tolerates unknown fields (`MathMapEntryShape`, `RedactionItemShape`,
`RenderPolicyPayloadShape`) are registered as *open*: the gate then enforces the
declared vocabulary as a superset and required keys, without rejecting extra
keys.

stdlib/typing-only; imports neither the shims nor the DTOs, so it can be
imported from anywhere in the rendering package.
"""

from __future__ import annotations

import types
import typing
from typing import Any, TypedDict


class ContractKeyMismatch(ValueError):
    """Bridge payload key set drifted from its declared shape."""


# --------------------------------------------------------------------------
# Shared leaf shapes (Rust `rendering_output::dto`).


class MathMapEntryShape(TypedDict, total=False):
    """`dto::MathMapEntry` — Rust reads `formula_text`/`latex` and tolerates
    unknown keys, so the entry is open."""

    formula_text: str
    latex: str


class LineBoxShape(TypedDict):
    """`dto::RenderLineBox` (`_line_box_to_dict` emits both keys)."""

    text: str
    bbox: list[float]


class TocEntryShape(TypedDict):
    """`dto::RenderTocEntry` (`_toc_entry_to_dict` always emits all five)."""

    title: str
    page_label: str
    bbox: list[float]
    number: str
    level: int


# --------------------------------------------------------------------------
# Boundary 1 — emitter page_specs (`output/typst/_native.py::_page_spec_to_dict`
# and `_block_to_dict`) → `rendering_output::dto::RenderPageSpec/RenderLayoutBlock`.
# `_block_to_dict` emits all 29 keys unconditionally, so the block shape is closed.


class EmitterBlockShape(TypedDict):
    block_id: str
    page_index: int
    background_rect: list[float]
    content_rect: list[float]
    content_kind: str
    content_text: str
    plain_text: str
    math_map: list[MathMapEntryShape]
    font_size_pt: float
    leading_em: float
    font_weight: str
    fit_to_box: bool
    fit_single_line: bool
    fit_min_font_size_pt: float
    fit_max_font_size_pt: float
    fit_min_leading_em: float
    fit_max_height_pt: float
    fit_target_width_pt: float
    fit_target_height_pt: float
    fit_shift_up_pt: float
    first_line_indent_pt: float
    justify_text: bool
    text_color: list[float]
    cover_fill: list[float]
    use_cover_fill: bool
    skip_reason: str
    preserve_line_breaks: bool
    preserved_line_boxes: list[LineBoxShape]
    toc_entries: list[TocEntryShape]


class EmitterPageSpecShape(TypedDict):
    page_index: int
    page_width_pt: float
    page_height_pt: float
    # Emitted unconditionally (None when no background); only genuinely optional
    # keys use `total=False`, so this stays required with a nullable value.
    background_pdf_path: str | None
    blocks: list[EmitterBlockShape]


# --------------------------------------------------------------------------
# Boundary 2 — redaction page_specs (`source/background/_native.py::
# render_page_spec_to_bridge`, the "fixed key order" hack) →
# `redaction::page_specs::RenderPageSpec`. Producer always emits all five keys.


class RedactionBlockShape(TypedDict):
    block_id: str
    background_rect: list[float]
    content_rect: list[float]
    content_kind: str
    content_text: str
    plain_text: str


class RedactionPageSpecShape(TypedDict):
    page_index: int
    page_width_pt: float
    page_height_pt: float
    background_pdf_path: str | None
    blocks: list[RedactionBlockShape]


# --------------------------------------------------------------------------
# Boundary 3 — translated_pages / RedactionItem (`json.dumps(translated_pages)`)
# → `redaction::dto::RedactionItem`. Items vary and Rust tolerates unknown keys,
# so the shape is open and every key optional.


class RenderPolicyPayloadShape(TypedDict, total=False):
    cleanup_mode: str
    overlay_fill: str


class RedactionItemShape(TypedDict, total=False):
    bbox: list[float] | None
    translated_text: str
    item_id: str
    source_item_id: str | None
    block_id: str
    protected_translated_text: str
    translation_unit_protected_translated_text: str
    translation_unit_translated_text: str
    source_text: str
    protected_source_text: str
    render_source_text: str
    block_kind: str
    block_type: str
    raw_block_type: str
    normalized_sub_type: str
    continuation_group: str | None
    continuation_group_id: str | None
    _force_visual_cover_only: bool
    _formula_guard_fragment: bool
    _formula_guard_fragment_index: int | None
    _visual_profile_fill: list[float] | None
    _render_cleanup_mode: str
    _render_overlay_fill: str
    _render_use_cover_fill: bool
    render_policy: RenderPolicyPayloadShape | None


# --------------------------------------------------------------------------
# Boundary 4 — visual_profile_fill_map (`visual_profile_fill_map`) →
# `HashMap<String, [f64; 3]>`. Dynamic item-id keys; values are 3-float RGB.


FillMapEntryShape = list[float]
FillMapShape = dict[str, FillMapEntryShape]

# --------------------------------------------------------------------------
# Boundary 5 — precleaned_page_indices (`sorted(...)`) → `HashSet<i32>`.


PrecleanedPageIndicesShape = list[int]

# --------------------------------------------------------------------------
# Boundary 6 — render bundle (`entrypoints/run_render_delegate.py::build_bundle`)
# → `rendering_orchestrator::bundle::RenderBundle`. All 12 keys emitted.


class RenderBundlePageMapShape(TypedDict):
    source_page_indices: list[int]


class RenderBundleShape(TypedDict):
    schema_version: str
    mode: str
    source_pdf: str
    output_pdf: str
    work_dir: str
    font_family: str
    redaction_strategy: str | None
    precleaned_page_indices: list[int]
    visual_profile_fill_map: dict[str, list[float]]
    page_map: RenderBundlePageMapShape
    translated_pages: dict[int, list[RedactionItemShape]]
    page_specs: list[EmitterPageSpecShape]


# --------------------------------------------------------------------------
# Boundary registry (id → shape) for the corpus generator and tests.


BRIDGE_BOUNDARIES: dict[str, type] = {
    "emitter_page_specs": EmitterPageSpecShape,
    "redaction_page_specs": RedactionPageSpecShape,
    "redaction_item": RedactionItemShape,
    "fill_map": FillMapShape,
    "precleaned_page_indices": PrecleanedPageIndicesShape,
    "render_bundle": RenderBundleShape,
}

#: Shapes whose wire payloads legitimately carry undeclared keys (the Rust DTO
#: tolerates unknown fields). For these the gate enforces required keys only;
#: every other shape is enforced as an exact key-set equality.
_OPEN_SHAPES: set[type] = set()


def _mark_open(shape: type) -> None:
    _OPEN_SHAPES.add(shape)


for _open_shape in (
    MathMapEntryShape,
    RedactionItemShape,
    RenderPolicyPayloadShape,
):
    _mark_open(_open_shape)


def _is_typeddict(shape: Any) -> bool:
    return (
        isinstance(shape, type)
        and issubclass(shape, dict)
        and hasattr(shape, "__required_keys__")
        and hasattr(shape, "__optional_keys__")
    )


def expected_keys(shape: Any) -> tuple[frozenset[str], frozenset[str]]:
    """`(required, optional)` key sets for a TypedDict shape.

    Container shapes (`fill_map` / `precleaned_page_indices`) have dynamic keys
    and return two empty sets.
    """
    if _is_typeddict(shape):
        return shape.__required_keys__, shape.__optional_keys__
    return frozenset(), frozenset()


def _union_members(shape: Any) -> list[Any]:
    origin = typing.get_origin(shape)
    if origin is typing.Union or origin is types.UnionType:
        return [member for member in typing.get_args(shape) if member is not type(None)]
    return []


def _structural(shape: Any) -> bool:
    """Whether `shape` demands recursive validation of the value."""
    if _is_typeddict(shape):
        return True
    origin = typing.get_origin(shape)
    if origin is dict or origin is list:
        return True
    return any(_structural(member) for member in _union_members(shape))


def _validate_value(value: Any, shape: Any) -> None:
    if _is_typeddict(shape):
        _assert_mapping(value, shape)
        return
    members = _union_members(shape)
    if members:
        if value is None:
            return
        for member in members:
            if _structural(member):
                _validate_value(value, member)
                return
        return
    origin = typing.get_origin(shape)
    args = typing.get_args(shape)
    if origin is list:
        if not isinstance(value, list):
            raise ContractKeyMismatch(
                f"expected list, got {type(value).__name__}: {value!r}"
            )
        for element in value:
            _validate_value(element, args[0])
        return
    if origin is dict:
        if not isinstance(value, dict):
            raise ContractKeyMismatch(
                f"expected dict, got {type(value).__name__}: {value!r}"
            )
        for element in value.values():
            _validate_value(element, args[1])


def _assert_mapping(value: Any, shape: type) -> None:
    if not isinstance(value, dict):
        raise ContractKeyMismatch(
            f"{shape.__name__}: expected dict, got {type(value).__name__}: {value!r}"
        )
    required = shape.__required_keys__
    declared = required | shape.__optional_keys__
    value_keys = set(value.keys())
    missing = required - value_keys
    if missing:
        raise ContractKeyMismatch(
            f"{shape.__name__}: missing required keys {sorted(missing)}"
        )
    extra = value_keys - declared
    if extra and shape not in _OPEN_SHAPES:
        raise ContractKeyMismatch(
            f"{shape.__name__}: unexpected keys {sorted(extra)}"
        )
    hints = typing.get_type_hints(shape)
    for key in value_keys:
        # Open shapes may carry undeclared keys the Rust DTO ignores; only
        # validate the recognized vocabulary.
        if key in hints:
            _validate_value(value[key], hints[key])


def assert_keys_match(value: Any, shape: Any) -> None:
    """Validate `value` against `shape`; raise `ContractKeyMismatch` on drift.

    A shape is a TypedDict (key-set gate + recursion into its annotations) or a
    container expression — `dict[str, list[float]]` for the fill map, `list[int]`
    for the precleaned indices.
    """
    _validate_value(value, shape)


def shape_to_schema(shape: Any) -> dict:
    """Serialize `shape` into a portable schema tree for the Rust strict gate.

    Node kinds:
      ``{"type": "obj", "fields": {key: node}, "required": [keys], "open": bool}``
      ``{"type": "arr", "elem": node}``
      ``{"type": "map", "value": node}``
      ``{"type": "any"}``

    ``open`` mirrors the Rust DTO's unknown-key tolerance (serde
    ``deny_unknown_fields`` off); closed shapes reject undeclared keys exactly
    like ``deny_unknown_fields``. The corpus generator records this next to each
    payload so the Rust test never duplicates the shape definitions.
    """
    if _is_typeddict(shape):
        required = shape.__required_keys__
        optional = shape.__optional_keys__
        hints = typing.get_type_hints(shape)
        fields = {key: shape_to_schema(hints[key]) for key in required | optional}
        return {
            "type": "obj",
            "fields": fields,
            "required": sorted(required),
            "open": shape in _OPEN_SHAPES,
        }
    members = _union_members(shape)
    if members:
        for member in members:
            if _structural(member):
                return shape_to_schema(member)
        return {"type": "any"}
    origin = typing.get_origin(shape)
    args = typing.get_args(shape)
    if origin is list:
        return {"type": "arr", "elem": shape_to_schema(args[0])}
    if origin is dict:
        return {"type": "map", "value": shape_to_schema(args[1])}
    return {"type": "any"}


__all__ = [
    "BRIDGE_BOUNDARIES",
    "ContractKeyMismatch",
    "EmitterBlockShape",
    "EmitterPageSpecShape",
    "FillMapShape",
    "RedactionBlockShape",
    "RedactionItemShape",
    "RedactionPageSpecShape",
    "RenderBundlePageMapShape",
    "RenderBundleShape",
    "assert_keys_match",
    "expected_keys",
    "shape_to_schema",
]
