"""Compatibility imports for the initial RenderPageProfile API.

New code should import from the focused modules:
- models.py for data structures
- kind.py for page-kind classification
"""

from services.rendering.analysis.profile.kind import classify_profile_kind
from services.rendering.analysis.profile.models import RenderPageKind
from services.rendering.analysis.profile.models import RenderPageProfile

__all__ = [
    "RenderPageKind",
    "RenderPageProfile",
    "classify_profile_kind",
]
