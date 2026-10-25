"""UTC-only conversions and invocation-scoped broker clocks."""
from _dekopon_time import time, monotonic, time_ns, _gmtime, _strftime
from collections import namedtuple as _namedtuple

struct_time = _namedtuple('struct_time', 'tm_year tm_mon tm_mday tm_hour tm_min tm_sec tm_wday tm_yday tm_isdst')
del _namedtuple

def gmtime(seconds=None):
    return struct_time(_gmtime(time() if seconds is None else seconds))

localtime = gmtime

def strftime(format, t=None):
    return _strftime(format, tuple(localtime() if t is None else t))

timezone = 0
altzone = 0
daylight = 0
tzname = ('UTC', 'UTC')
