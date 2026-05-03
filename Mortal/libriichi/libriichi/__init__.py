from .libriichi import *
import sys as _sys

__doc__ = libriichi.__doc__
if hasattr(libriichi, "__all__"):
    __all__ = libriichi.__all__

# Register submodules as libriichi.X (not just libriichi.libriichi.X)
for _name in ('consts', 'state', 'dataset', 'arena', 'stat', 'mjai'):
    if hasattr(libriichi, _name):
        _sys.modules[f'{__name__}.{_name}'] = getattr(libriichi, _name)
del _name, _sys
