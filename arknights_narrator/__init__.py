"""Natural-language rendering for deterministic player training advice."""

from .adapter import Narrator, NarratorConfig, NarratorResult
from .cache import Admission, AdmissionStatus, RedisKeys, admit, content_hmac
from .mapping import CanonicalMapping, MappingError
from .projection import NarrativeInput, project_report

__all__ = [
    "CanonicalMapping",
    "Admission",
    "AdmissionStatus",
    "MappingError",
    "NarrativeInput",
    "Narrator",
    "NarratorConfig",
    "NarratorResult",
    "RedisKeys",
    "admit",
    "content_hmac",
    "project_report",
]
