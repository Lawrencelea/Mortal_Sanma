""" libriichi3p — proxies to libriichi, which supports sanma (version 5) """
import sys as _sys
import libriichi as _libriichi

# Mirror all public names
for _n in dir(_libriichi):
    if not _n.startswith('_'):
        globals()[_n] = getattr(_libriichi, _n)

# Register submodules so 'from libriichi3p.consts import X' works
for _name in ('consts', 'state', 'dataset', 'arena', 'stat', 'mjai'):
    _sub = _sys.modules.get(f'libriichi.{_name}') or getattr(_libriichi, _name, None)
    if _sub is not None:
        _sys.modules[f'{__name__}.{_name}'] = _sub

del _n, _name, _sub, _libriichi
