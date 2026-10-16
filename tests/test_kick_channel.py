"""Run with Python + yt-dlp, or the local yt-dlp test harness."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock

from yt_dlp.utils import ExtractorError

path = Path(__file__).resolve().parents[1] / 'engine_plugins/oval/yt_dlp_plugins/extractor/oval_kick.py'
spec = importlib.util.spec_from_file_location('oval_kick_tested', path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class KickChannelTests(unittest.TestCase):
    def setUp(self):
        self.ie = module.OvalKickChannelIE()

    def test_only_collections_match(self):
        for url in ('https://kick.com/lacobraaa/videos', 'https://www.kick.com/lacobraaa/clips/?sort=date'):
            self.assertTrue(self.ie.suitable(url))
        for url in ('https://kick.com/lacobraaa', 'https://kick.com/lacobraaa/videos/uuid',
                    'https://kick.com/lacobraaa/clips/clip_123', 'https://kick.com.evil/lacobraaa/videos'):
            self.assertFalse(self.ie.suitable(url))

    def test_vod_has_canonical_id_thumbnail_and_seconds(self):
        row = {'video': {'uuid': 'vod-id'}, 'session_title': 'Video', 'duration': 123000,
               'thumbnail': {'src': 'https://images.kick.com/thumb.webp'}}
        entry = self.ie._entry(row, 'channel', 'videos')
        self.assertEqual(entry['id'], 'vod-id')
        self.assertEqual(entry['url'], 'https://kick.com/channel/videos/vod-id')
        self.assertEqual(entry['duration'], 123.0)
        self.assertEqual(entry['thumbnail'], row['thumbnail']['src'])
        for flag in ('is_private', 'is_pruned', 'deleted_at'):
            self.assertIsNone(self.ie._entry({**row, 'video': {'uuid': 'vod-id', flag: True}}, 'channel', 'videos'))
        self.assertIsNone(self.ie._entry({**row, 'is_live': True}, 'channel', 'videos'))

    def test_clips_paginate_and_deduplicate(self):
        a, b = {'id': 'clip_a', 'duration': 17}, {'id': 'clip_b', 'duration': 30}
        self.ie._call_api = Mock(side_effect=[
            {'clips': [a], 'nextCursor': 'cursor-a'},
            {'clips': [a, b], 'nextCursor': None},
        ])
        entries = list(self.ie._entries('channel', 'clips'))
        self.assertEqual([e['id'] for e in entries], ['clip_a', 'clip_b'])
        self.assertEqual(entries[0]['duration'], 17.0)
        self.assertEqual(self.ie._call_api.call_args.kwargs['query'], {'cursor': 'cursor-a'})

    def test_repeated_cursor_stops(self):
        self.ie._call_api = Mock(side_effect=[
            {'clips': [{'id': 'clip_a'}], 'nextCursor': 'same'},
            {'clips': [{'id': 'clip_b'}], 'nextCursor': 'same'},
        ])
        self.assertEqual(len(list(self.ie._entries('channel', 'clips'))), 2)
        self.assertEqual(self.ie._call_api.call_count, 2)

    def test_empty_list_and_malformed_response(self):
        self.ie._call_api = Mock(return_value=[])
        self.assertEqual(list(self.ie._entries('channel', 'videos')), [])
        self.ie._call_api = Mock(return_value={'unexpected': []})
        with self.assertRaises(ExtractorError):
            list(self.ie._entries('channel', 'videos'))
        self.ie._call_api = Mock(return_value={'clips': [{'id': 'clip_a'}], 'nextCursor': {'invalid': 1}})
        with self.assertRaises(ExtractorError):
            list(self.ie._entries('channel', 'clips'))


class KickVodTests(unittest.TestCase):
    SOURCE = ('https://stream.kick.com/3c81249a5ce0/ivs/v1/196233775518/1jxRW8srP4xt/'
              '2026/10/14/18/17/FevkIoASjqXY/media/hls/master.m3u8')

    def test_both_kinds_of_id_match(self):
        ie = module.OvalKickVodIE()
        for vod in ('01a13ba2-61a0-73a5-bb58-9ca6d0cb58b4', 'a97d92fb-a32e-4fde-aea1-a79f5b4279ee'):
            self.assertTrue(ie.suitable(f'https://kick.com/lacobraaa/videos/{vod}'))
        self.assertFalse(ie.suitable('https://kick.com/lacobraaa/videos'))

    def test_page_video_object(self):
        page = ('<script type="application/ld+json">{"@graph":[{"@type":"WebPage"},'
                '{"@type":"VideoObject","name":"Vod","contentUrl":"%s","duration":"PT10H32M35S"}]}</script>' % self.SOURCE)
        video = module.video_object(page)
        self.assertEqual(video['name'], 'Vod')
        self.assertEqual(video['contentUrl'], self.SOURCE)
        self.assertIsNone(module.video_object('<html></html>'))

    def test_subtitles_and_thumbnails_come_from_the_stream(self):
        master = ('#EXTM3U\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subtitles",NAME="Spanish",'
                  'URI="subtitles/es/playlist.m3u8",LANGUAGE="es"\n#EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID="1080p60"\n')
        subs = module.hls_subtitles(master, self.SOURCE)
        self.assertEqual(list(subs), ['es'])
        self.assertTrue(subs['es'][0]['url'].endswith('/media/hls/subtitles/es/playlist.m3u8'))
        thumbs = module.stream_thumbnails(self.SOURCE)
        self.assertEqual(thumbs[-1]['url'],
                         'https://images.kick.com/video_thumbnails/1jxRW8srP4xt/FevkIoASjqXY/1080.webp')
        self.assertEqual(module.stream_thumbnails('https://example.com/x.m3u8'), [])


if __name__ == '__main__':
    unittest.main()
