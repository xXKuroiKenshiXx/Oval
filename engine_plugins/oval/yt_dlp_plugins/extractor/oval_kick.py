"""Kick additions on top of yt-dlp's built-in extractors.

- Channel listings (videos and clips), using yt-dlp's Kick HTTP client.
- VODs: Kick's website now links videos by an ID its old API doesn't know
  (404). Those are read from the page itself, and every VOD gets Kick's
  automatic subtitles from its HLS stream.

Clips and live channels continue through yt-dlp's built-in extractors.
"""

import json
import re
import urllib.parse

from yt_dlp.extractor.kick import KickBaseIE, KickClipIE, KickVODIE
from yt_dlp.networking.exceptions import HTTPError
from yt_dlp.utils import ExtractorError, float_or_none, parse_duration, parse_iso8601, url_or_none

__all__ = ['OvalKickChannelIE', 'OvalKickVodIE']

# Kick names stream languages in English.
LANGUAGES = {
    'arabic': 'ar', 'chinese': 'zh', 'dutch': 'nl', 'english': 'en', 'french': 'fr', 'german': 'de',
    'italian': 'it', 'japanese': 'ja', 'korean': 'ko', 'polish': 'pl', 'portuguese': 'pt',
    'russian': 'ru', 'spanish': 'es', 'turkish': 'tr',
}


def video_object(webpage):
    """The schema.org VideoObject that Kick puts in every VOD page."""
    for raw in re.findall(r'<script[^>]+type="application/ld\+json"[^>]*>(.+?)</script>', webpage, re.S):
        try:
            data = json.loads(raw)
        except ValueError:
            continue
        for node in data.get('@graph', [data]) if isinstance(data, dict) else data:
            if isinstance(node, dict) and node.get('@type') == 'VideoObject' and node.get('contentUrl'):
                return node
    return None


def stream_thumbnails(source):
    """Kick keeps VOD thumbnails on images.kick.com under the same IDs as the
    stream (.../ivs/v1/<account>/<content>/<y>/<m>/<d>/<h>/<min>/<session>/...)."""
    match = re.search(r'/ivs/v1/\d+/(\w+)/\d{4}/\d+/\d+/\d+/\d+/(\w+)/', source or '')
    if not match:
        return []
    base = 'https://images.kick.com/video_thumbnails/{}/{}'.format(*match.groups())
    return [{'url': f'{base}/{size}.webp', 'height': size, 'preference': size}
            for size in (480, 720, 1080)]


def hls_subtitles(master, master_url):
    """Subtitle tracks listed in an HLS master playlist, as yt-dlp subtitles."""
    subtitles = {}
    for line in master.splitlines():
        if not line.startswith('#EXT-X-MEDIA:') or 'TYPE=SUBTITLES' not in line:
            continue
        attrs = dict(re.findall(r'([A-Z-]+)="([^"]*)"', line))
        if attrs.get('URI') and attrs.get('LANGUAGE'):
            subtitles.setdefault(attrs['LANGUAGE'], []).append({
                'url': urllib.parse.urljoin(master_url, attrs['URI']),
                'ext': 'vtt',
                'protocol': 'm3u8_native',
                'name': attrs.get('NAME'),
            })
    return subtitles


class OvalKickChannelIE(KickBaseIE):
    IE_NAME = 'oval:kick:channel'
    _VALID_URL = r'https?://(?:www\.)?kick\.com/(?P<id>[\w-]+)/(?P<section>videos|clips)/?(?:[?#].*)?$'

    def _entry(self, item, channel, section):
        if not isinstance(item, dict):
            return None
        if section == 'videos':
            video = item.get('video') or {}
            if not isinstance(video, dict) or item.get('is_live') or video.get('is_private') or video.get('is_pruned') or video.get('deleted_at'):
                return None
            video_id = video.get('uuid')
            if not isinstance(video_id, str) or not video_id:
                return None
            thumbnail = item.get('thumbnail')
            if isinstance(thumbnail, dict):
                thumbnail = thumbnail.get('src') or thumbnail.get('url')
            return self.url_result(
                f'https://kick.com/{channel}/videos/{video_id}', KickVODIE,
                video_id=video_id, video_title=item.get('session_title') or video_id,
                thumbnail=url_or_none(thumbnail),
                duration=float_or_none(item.get('duration'), scale=1000),
                channel=channel, uploader=channel)
        clip_id = item.get('id')
        if not isinstance(clip_id, str) or not clip_id.startswith('clip_') or item.get('privacy', 'public') != 'public':
            return None
        return self.url_result(
            f'https://kick.com/{channel}/clips/{clip_id}', KickClipIE,
            video_id=clip_id, video_title=item.get('title') or clip_id,
            thumbnail=url_or_none(item.get('thumbnail_url')),
            duration=float_or_none(item.get('duration')),
            channel=channel, uploader=channel)

    def _entries(self, channel, section):
        cursor = None
        cursors, seen = set(), set()
        while True:
            query = {'cursor': cursor} if cursor else {}
            data = self._call_api(
                f'v2/channels/{channel}/{section}', channel,
                note=f'Downloading Kick {section}', query=query)
            if isinstance(data, list):
                rows, next_cursor = data, None
            elif isinstance(data, dict):
                rows = data.get(section, data.get('data'))
                next_cursor = data.get('nextCursor', data.get('next_cursor'))
            else:
                rows, next_cursor = None, None
            if not isinstance(rows, list):
                raise ExtractorError('Kick returned an invalid channel listing', expected=True)
            added = 0
            for item in rows:
                entry = self._entry(item, channel, section)
                if entry and entry['id'] not in seen:
                    seen.add(entry['id'])
                    added += 1
                    yield entry
            # Avoid loops if Kick repeats a cursor or the same page.
            if not rows or not next_cursor or not added:
                break
            if not isinstance(next_cursor, str):
                raise ExtractorError('Kick returned an invalid pagination cursor', expected=True)
            if next_cursor in cursors:
                break
            cursors.add(next_cursor)
            cursor = next_cursor

    def _real_extract(self, url):
        match = self._match_valid_url(url)
        channel, section = match.group('id', 'section')
        return self.playlist_result(
            self._entries(channel, section), playlist_id=f'{channel}-{section}',
            playlist_title=f'{channel} - {section}', uploader=channel, channel=channel)


class OvalKickVodIE(KickVODIE):
    IE_NAME = 'oval:kick:vod'
    _VALID_URL = r'https?://(?:www\.)?kick\.com/(?P<channel>[\w-]+)/videos/(?P<id>[\da-f]{8}-(?:[\da-f]{4}-){3}[\da-f]{12})'

    def _from_page(self, url, channel, video_id):
        webpage = self._download_webpage(url, video_id, impersonate=True)
        video = video_object(webpage)
        if not video:
            raise ExtractorError('Kick did not describe this video', expected=True)
        thumbnail = video.get('thumbnailUrl')
        if isinstance(thumbnail, list):
            thumbnail = thumbnail[0] if thumbnail else None
        return {
            'id': video_id,
            'title': video.get('name') or video_id,
            'formats': self._extract_m3u8_formats(video['contentUrl'], video_id, 'mp4'),
            'thumbnail': url_or_none(thumbnail),
            'duration': parse_duration(video.get('duration')),
            'timestamp': parse_iso8601(video.get('uploadDate')),
            'channel': channel,
            'uploader': channel,
        }

    def _listing(self, channel, source):
        """This video's row in the channel's recent videos (best effort): it
        has the stream's language and thumbnail."""
        try:
            rows = self._call_api(f'v2/channels/{channel}/videos', channel, note='Downloading Kick videos', fatal=False)
        except ExtractorError:
            return {}
        for row in rows if isinstance(rows, list) else []:
            if isinstance(row, dict) and row.get('source') == source:
                return row
        return {}

    def _real_extract(self, url):
        channel, video_id = self._match_valid_url(url).group('channel', 'id')
        try:
            info = super()._real_extract(url)
        except ExtractorError as e:
            if not isinstance(e.cause, HTTPError) or e.cause.status != 404:
                raise
            info = self._from_page(url, channel, video_id)
        source = next((f.get('manifest_url') for f in info.get('formats') or [] if f.get('manifest_url')), None)
        if source:
            master = self._download_webpage(source, video_id, note='Downloading subtitle list', fatal=False)
            if master:
                info['subtitles'] = hls_subtitles(master, source)
            # The page's own thumbnail sits behind a bot check; these don't.
            info['thumbnails'] = stream_thumbnails(source) + (
                [{'url': info['thumbnail'], 'preference': -1}] if info.get('thumbnail') else [])
            row = self._listing(channel, source)
            info['language'] = LANGUAGES.get(str(row.get('language') or '').lower())

        return info
