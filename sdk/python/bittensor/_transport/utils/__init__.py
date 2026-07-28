import importlib


def import_json_lib():
    """The fastest available JSON implementation (they share the stdlib API)."""
    libs = ["ujson", "orjson", "simplejson", "json"]

    for lib in libs:
        try:
            return importlib.import_module(lib)
        except ImportError:
            continue

    raise ImportError("None of the specified JSON libraries are installed.")


json = import_json_lib()
