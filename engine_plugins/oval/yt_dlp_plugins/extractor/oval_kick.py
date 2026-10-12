"""Public Kick channel listings, using yt-dlp's Kick HTTP client.

Only collection URLs match this extractor. Individual VODs, clips and live
channels continue through yt-dlp's maintained built-in Kick extractors.
"""

from yt_dlp.extractor.kick import KickBaseIE, KickClipIE, KickVODIE
from yt_dlp.utils import ExtractorError, float_or_none, url_or_none

__all__ = ['OvalKickChannelIE']


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
