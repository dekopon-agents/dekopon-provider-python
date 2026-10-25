"""UTC-only conversions and invocation-scoped broker clocks."""
from _dekopon_time import time, monotonic, _gmtime, _strftime
from collections import namedtuple as _namedtuple

class struct_time(_namedtuple('struct_time', 'tm_year tm_mon tm_mday tm_hour tm_min tm_sec tm_wday tm_yday tm_isdst')):
    __slots__ = ()
    tm_gmtoff = 0
    tm_zone = 'UTC'

    def __new__(cls, iterable):
        return super().__new__(cls, *tuple(iterable))
del _namedtuple

def gmtime(seconds=None):
    return struct_time(_gmtime(time() if seconds is None else float(seconds)))

localtime = gmtime

def strftime(format, t=None):
    return _strftime(format, tuple(localtime() if t is None else t))

