from __future__ import annotations


def resolve_page_range(total_pages: int, start_page: int, end_page: int) -> tuple[int, int]:
    start = max(0, start_page)
    stop = total_pages - 1 if end_page < 0 else min(end_page, total_pages - 1)
    if start > stop:
        raise RuntimeError(
            f"Invalid page range: start_page={start_page}, end_page={end_page} "
            f"(clamped to start={start}, stop={stop} for total_pages={total_pages})"
        )
    return start, stop


__all__ = ["resolve_page_range"]
