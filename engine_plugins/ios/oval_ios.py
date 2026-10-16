"""Runs yt-dlp inside the iOS app, where no other program may be started.

The Swift side of the `oval-engine` plugin starts Python once and calls
`setup()` with the addresses of three native functions:

- emit(job, stream, line): one line of output for a job (1 = stdout, 2 = stderr,
  0 = the exit code; job 0 carries replies to setup and update)
- merge(video, audio, out): joins an MP4 video and an M4A audio with AVFoundation
- run_js(script): runs JavaScript with JavaScriptCore and returns a JSON envelope
  ({"out": ...} or {"error": ...}) that `release()` frees afterwards

Then every download is `run(job, args)` on its own thread, exactly like a
yt-dlp process on desktop: it prints the same lines and returns an exit code.
Without subprocesses there is no FFmpeg; merging is the only post-processing
that iOS needs, and AVFoundation does it.
"""

import ctypes
import json
import os
import sys
import threading
import zipfile

_native = {}
_jobs = threading.local()
_canceled = set()
_threads = {}
_lock = threading.Lock()
_home = None


class _Lines:
    """sys.stdout / sys.stderr: sends each finished line to the job that runs
    on the current thread."""

    encoding = "utf-8"
    errors = "replace"
    mode = "w"

    def __init__(self, stream):
        self.stream = stream

    def write(self, text):
        job = getattr(_jobs, "id", 0)
        if job and job in _canceled:
            raise KeyboardInterrupt
        buf = getattr(_jobs, "buf", None)
        if buf is None:
            buf = _jobs.buf = {1: "", 2: ""}
        buf[self.stream] += text.replace("\r", "\n")
        *lines, buf[self.stream] = buf[self.stream].split("\n")
        for line in lines:
            if line.strip():
                _native["emit"](job, self.stream, line.encode("utf-8", "replace"))
        return len(text)

    def flush(self):
        pass

    def finish(self):
        buf = getattr(_jobs, "buf", None) or {}
        for stream, rest in buf.items():
            if rest.strip():
                _native["emit"](getattr(_jobs, "id", 0), stream, rest.encode("utf-8", "replace"))
        _jobs.buf = {1: "", 2: ""}

    def isatty(self):
        return False


def setup(emit, merge, run_js, release, home):
    """Called once, right after Python starts."""
    global _home
    _home = home
    _native["emit"] = ctypes.CFUNCTYPE(None, ctypes.c_int64, ctypes.c_int32, ctypes.c_char_p)(emit)
    _native["merge"] = ctypes.CFUNCTYPE(ctypes.c_int32, ctypes.c_char_p, ctypes.c_char_p, ctypes.c_char_p)(merge)
    _native["run_js"] = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_char_p)(run_js)
    _native["release"] = ctypes.CFUNCTYPE(None, ctypes.c_void_p)(release)
    sys.stdout = _Lines(1)
    sys.stderr = _Lines(2)
    # A yt-dlp fetched by update() wins over the one inside the app.
    current = _current_update()
    if current:
        sys.path.insert(0, current)
    import yt_dlp.version

    _patch()
    return yt_dlp.version.__version__


# ---------------------------------------------------------------- yt-dlp on iOS


def _patch():
    from yt_dlp.postprocessor import ffmpeg
    from yt_dlp.utils import PostProcessingError, prepend_extension

    def merge(self, info):
        filename = info["filepath"]
        temp = prepend_extension(filename, "temp")
        video = audio = None
        for fmt in info["requested_formats"]:
            if fmt.get("vcodec") != "none" and video is None:
                video = fmt["filepath"]
            elif fmt.get("acodec") != "none" and audio is None:
                audio = fmt["filepath"]
        if not video or not audio:
            raise PostProcessingError("Formatos que no se pueden unir")
        self.to_screen(f'Merging formats into "{filename}"')
        code = _native["merge"](os.fsencode(video), os.fsencode(audio), os.fsencode(temp))
        if code != 0:
            raise PostProcessingError("No se pudieron unir el vídeo y el audio")
        os.replace(temp, filename)
        return info["__files_to_merge"], info

    # Only merging is possible (and claimed); every other FFmpeg step stays off.
    ffmpeg.FFmpegMergerPP.available = property(lambda self: True)
    ffmpeg.FFmpegMergerPP.run = merge
    _register_js_runtime()


def _register_js_runtime():
    from yt_dlp.extractor.youtube.jsc._builtin.ejs import EJSBaseJCP
    from yt_dlp.extractor.youtube.jsc._registry import _jsc_providers
    from yt_dlp.extractor.youtube.jsc.provider import (
        JsChallengeProviderError,
        register_preference,
        register_provider,
    )

    class JavaScriptCoreJCP(EJSBaseJCP):
        PROVIDER_VERSION = "1.0.0"
        JS_RUNTIME_NAME = "javascriptcore"
        BUG_REPORT_LOCATION = "https://github.com/xXKuroiKenshiXx/Oval/issues"

        def is_available(self):
            return self._available

        def _run_js_runtime(self, stdin, /):
            ptr = _native["run_js"](stdin.encode("utf-8"))
            if not ptr:
                raise JsChallengeProviderError("JavaScriptCore no respondió")
            try:
                answer = json.loads(ctypes.string_at(ptr).decode("utf-8"))
            finally:
                _native["release"](ptr)
            if "error" in answer:
                raise JsChallengeProviderError(f"JavaScriptCore: {answer['error']}")
            return answer["out"]

    if "JavaScriptCore" not in _jsc_providers.value:
        register_provider(JavaScriptCoreJCP)
        register_preference(JavaScriptCoreJCP)(lambda provider, requests: 1000)


# ---------------------------------------------------------------- jobs


def run(job, args):
    """Runs yt-dlp with `args` (a JSON list) and returns its exit code."""
    import yt_dlp

    _jobs.id = job
    with _lock:
        _threads[job] = threading.get_ident()
    code = 1
    try:
        yt_dlp.main(json.loads(args))
        code = 0
    except SystemExit as e:
        code = e.code if isinstance(e.code, int) else (0 if e.code is None else 1)
        if isinstance(e.code, str) and e.code.strip():
            _say(job, e.code.strip())
    except KeyboardInterrupt:
        code = 130
    except BaseException as e:  # noqa: BLE001 - everything must reach the app
        _say(job, f"ERROR: {e}")
    finally:
        sys.stderr.finish()
        sys.stdout.finish()
        with _lock:
            _threads.pop(job, None)
            canceled = job in _canceled
            _canceled.discard(job)
        _jobs.id = 0
    return 130 if canceled else code


def run_and_report(job, args):
    """`run`, then the exit code as the job's last message (stream 0)."""
    code = run(job, args)
    _native["emit"](job, 0, str(code).encode())


def _native_reply(text):
    """The answer to setup() or update(), read by the Swift side."""
    _native["emit"](0, 0, str(text).encode("utf-8", "replace"))


def _say(job, line):
    """A stderr line that goes out even while the job is being canceled."""
    _native["emit"](job, 2, line.encode("utf-8", "replace"))


def kill(job):
    """Stops a running job at its next line of output or Python instruction."""
    with _lock:
        _canceled.add(job)
        ident = _threads.get(job)
    if ident is not None:
        ctypes.pythonapi.PyThreadState_SetAsyncExc(ctypes.c_ulong(ident), ctypes.py_object(KeyboardInterrupt))


# ---------------------------------------------------------------- updates


def _updates_dir():
    return os.path.join(_home, "yt-dlp")


def _current_update():
    if not _home:
        return None
    try:
        with open(os.path.join(_updates_dir(), "current"), encoding="utf-8") as f:
            path = os.path.join(_updates_dir(), f.read().strip())
        return path if os.path.isdir(os.path.join(path, "yt_dlp")) else None
    except OSError:
        return None


def update():
    """Fetches the latest stable yt-dlp (and the JavaScript solver it asks
    for) from PyPI. It is used from the next start of the app."""
    import re
    import shutil
    import urllib.request

    import yt_dlp.version

    def pypi(name, version=None):
        url = f"https://pypi.org/pypi/{name}/{version + '/' if version else ''}json"
        with urllib.request.urlopen(url, timeout=30) as r:
            data = json.load(r)
        wheel = next(u for u in data["urls"] if u["filename"].endswith("-py3-none-any.whl"))
        return data["info"]["version"], wheel["url"]

    latest, url = pypi("yt-dlp")
    if latest == yt_dlp.version.__version__:
        return latest
    target = os.path.join(_updates_dir(), latest)
    partial = target + ".part"
    shutil.rmtree(partial, ignore_errors=True)
    os.makedirs(partial)
    wheel = os.path.join(partial, "wheel.zip")
    urllib.request.urlretrieve(url, wheel)
    with zipfile.ZipFile(wheel) as z:
        z.extractall(partial)
        meta = next((n for n in z.namelist() if n.endswith(".dist-info/METADATA")), None)
        requires = z.read(meta).decode("utf-8", "replace") if meta else ""
    os.remove(wheel)
    pinned = re.search(r"^Requires-Dist: yt-dlp-ejs==([\w.]+)", requires, re.M)
    if pinned:
        _, ejs_url = pypi("yt-dlp-ejs", pinned.group(1))
        ejs = os.path.join(partial, "ejs.zip")
        urllib.request.urlretrieve(ejs_url, ejs)
        with zipfile.ZipFile(ejs) as z:
            z.extractall(partial)
        os.remove(ejs)
    shutil.rmtree(target, ignore_errors=True)
    os.rename(partial, target)
    with open(os.path.join(_updates_dir(), "current"), "w", encoding="utf-8") as f:
        f.write(latest)
    # Keep only the version in use.
    for name in os.listdir(_updates_dir()):
        if name not in (latest, "current"):
            shutil.rmtree(os.path.join(_updates_dir(), name), ignore_errors=True)
    return latest
