"""Exercise Oval's actual format selector with yt-dlp, without downloading files."""
import re
import unittest
from pathlib import Path

from yt_dlp import YoutubeDL
from yt_dlp.utils import DownloadError, ExtractorError

source = (Path(__file__).resolve().parents[1] / 'src/downloads.rs').read_text(encoding='utf-8')
selector = re.search(r'args\[format\] = "(bv\*[^"\n]+)"', source).group(1)
sort = re.search(r'Some\(h\) if h == "light" => "([^"]+)"', source).group(1)


def video(name, height, bitrate):
    return {'format_id': name, 'url': f'https://example.invalid/{name}.mp4',
            'ext': 'mp4', 'vcodec': 'avc1.64001f', 'acodec': 'none',
            'height': height, 'width': height * 16 // 9, 'tbr': bitrate, 'fps': 30}


def select(formats):
    formats = formats + [{'format_id': 'audio', 'url': 'https://example.invalid/audio.m4a',
                         'ext': 'm4a', 'vcodec': 'none', 'acodec': 'mp4a.40.2', 'abr': 96}]
    with YoutubeDL({'quiet': True, 'no_warnings': True, 'format': selector,
                    'format_sort': sort.split(',')}) as ydl:
        return ydl.process_ie_result({'id': 'fixture', 'title': 'Fixture', 'extractor': 'generic',
                                      'extractor_key': 'Generic', 'formats': formats}, download=False)


class LightDownloadTests(unittest.TestCase):
    def test_caps_resolution_and_prefers_lower_bitrate(self):
        result = select([video('fullhd', 1080, 4000), video('hd-large', 720, 2500),
                         video('hd-small', 720, 800), video('sd', 360, 400)])
        self.assertEqual(result['requested_formats'][0]['format_id'], 'hd-small')

    def test_uses_lower_resolution_when_720_is_unavailable(self):
        result = select([video('fullhd', 1080, 4000), video('sd', 480, 600)])
        self.assertEqual(result['requested_formats'][0]['height'], 480)

    def test_does_not_silently_download_an_oversized_resolution(self):
        with self.assertRaises((DownloadError, ExtractorError)):
            select([video('fullhd', 1080, 4000)])


if __name__ == '__main__':
    unittest.main()
