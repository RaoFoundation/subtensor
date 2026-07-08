"""A minimal least-recently-used cache."""

from __future__ import annotations

from collections import OrderedDict
from typing import Any, Hashable, Optional


class LRUCache:
    def __init__(self, max_size: int):
        self.max_size = max_size
        self._data: OrderedDict = OrderedDict()

    def set(self, key: Hashable, value: Any) -> None:
        if key in self._data:
            self._data.move_to_end(key)
        self._data[key] = value
        if len(self._data) > self.max_size:
            self._data.popitem(last=False)

    def get(self, key: Hashable) -> Optional[Any]:
        if key in self._data:
            self._data.move_to_end(key)
            return self._data[key]
        return None
