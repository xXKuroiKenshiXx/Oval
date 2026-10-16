//! Turns a WebVTT subtitle track into readable transcripts: Markdown, plain
//! text, SRT or a clean VTT. YouTube's automatic captions repeat every line
//! while the next one scrolls in, so lines are de-duplicated first.

use std::fmt::Write;

use serde::Deserialize;

use crate::downloads::Section;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// Paragraphs of plain text under a small header.
    Md,
    /// Same, with a clickable timestamp before each paragraph.
    MdTimes,
    Txt,
    Srt,
    Vtt,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Md | Format::MdTimes => "md",
            Format::Txt => "txt",
            Format::Srt => "srt",
            Format::Vtt => "vtt",
        }
    }
}

/// What goes in the Markdown header.
#[derive(Default)]
pub struct Meta<'a> {
    pub title: &'a str,
    pub channel: &'a str,
    pub date: &'a str,
    pub url: &'a str,
    pub note: &'a str,
}

/// One caption line and when it appears, in seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Every distinct caption line, in order.
pub fn parse_vtt(vtt: &str) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut timing: Option<(f64, f64)> = None;
    // Whether the current cue already added a line (a cue can span two rows).
    let mut added = false;
    for raw in vtt.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            timing = None;
            continue;
        }
        if let Some((a, b)) = raw.split_once("-->") {
            let end = b.split_whitespace().next().unwrap_or("");
            timing = parse_time(a.trim()).zip(parse_time(end));
            added = false;
            continue;
        }
        let Some((start, end)) = timing else { continue };
        let text = strip_tags(raw);
        if text.is_empty() {
            continue;
        }
        match lines.last_mut() {
            // The same line still on screen: just extend it.
            Some(last) if last.text == text || last.text.ends_with(&format!(" {text}")) => {
                last.end = last.end.max(end)
            }
            Some(last) if added => {
                last.text.push(' ');
                last.text.push_str(&text);
            }
            _ => {
                lines.push(Line { start, end, text });
                added = true;
            }
        }
    }
    lines
}

/// "00:01:02.500" or "01:02.500" in seconds.
fn parse_time(s: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in s.replace(',', ".").split(':') {
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(total)
}

/// Removes `<c>`, `<00:00:01.000>` and similar tags, and HTML entities.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Keeps the lines that start inside `section` (or just before it).
pub fn clip(lines: Vec<Line>, section: Option<Section>) -> Vec<Line> {
    match section {
        Some(s) => lines
            .into_iter()
            .filter(|l| l.start >= s.start - 1.0 && l.start < s.end)
            .collect(),
        None => lines,
    }
}

/// Paragraphs of about `every` seconds, broken after a sentence when possible.
fn paragraphs(lines: &[Line], every: f64) -> Vec<(f64, String)> {
    let mut out: Vec<(f64, String)> = Vec::new();
    let mut current: Option<(f64, String)> = None;
    for line in lines {
        let (start, text) = current.get_or_insert_with(|| (line.start, String::new()));
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&line.text);
        let long = line.end - *start >= every;
        let sentence = text.ends_with(['.', '?', '!', '…']);
        if (long && sentence) || line.end - *start >= every * 2.0 {
            out.extend(current.take());
        }
    }
    out.extend(current);
    out
}

fn stamp(sec: f64) -> String {
    let s = sec.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn cue_time(sec: f64, sep: char) -> String {
    let ms = (sec.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{sep}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// A link that opens the video at `sec` (YouTube and most sites accept `t=`).
fn at(url: &str, sec: f64) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}t={}s", sec.max(0.0) as u64)
}

/// Renders the transcript. Subtitle formats start at zero when `offset` is the
/// clip start, so they line up with a clip downloaded from the same range.
pub fn render(lines: &[Line], format: Format, meta: &Meta, offset: f64) -> String {
    let mut out = String::new();
    match format {
        Format::Md | Format::MdTimes => {
            let _ = writeln!(out, "# {}\n", meta.title.trim());
            let mut info: Vec<String> = Vec::new();
            for part in [meta.channel, meta.date] {
                if !part.is_empty() {
                    info.push(part.to_string());
                }
            }
            if !meta.url.is_empty() {
                info.push(format!("<{}>", meta.url));
            }
            if !info.is_empty() {
                let _ = writeln!(out, "{}\n", info.join(" · "));
            }
            if !meta.note.trim().is_empty() {
                let _ = writeln!(out, "## TL;DR\n\n{}\n", meta.note.trim());
            }
            let _ = writeln!(out, "## Transcripción\n");
            for (start, text) in paragraphs(lines, 45.0) {
                if format == Format::MdTimes && !meta.url.is_empty() {
                    let _ = writeln!(out, "[{}]({}) {text}\n", stamp(start), at(meta.url, start));
                } else if format == Format::MdTimes {
                    let _ = writeln!(out, "**{}** {text}\n", stamp(start));
                } else {
                    let _ = writeln!(out, "{text}\n");
                }
            }
        }
        Format::Txt => {
            for (_, text) in paragraphs(lines, 45.0) {
                let _ = writeln!(out, "{text}\n");
            }
        }
        Format::Srt => {
            for (i, l) in lines.iter().enumerate() {
                let end = lines
                    .get(i + 1)
                    .map_or(l.end, |n| n.start.min(l.end).max(l.start));
                let _ = writeln!(
                    out,
                    "{}\n{} --> {}\n{}\n",
                    i + 1,
                    cue_time(l.start - offset, ','),
                    cue_time(end - offset, ','),
                    l.text
                );
            }
        }
        Format::Vtt => {
            out.push_str("WEBVTT\n\n");
            for (i, l) in lines.iter().enumerate() {
                let end = lines
                    .get(i + 1)
                    .map_or(l.end, |n| n.start.min(l.end).max(l.start));
                let _ = writeln!(
                    out,
                    "{} --> {}\n{}\n",
                    cue_time(l.start - offset, '.'),
                    cue_time(end - offset, '.'),
                    l.text
                );
            }
        }
    }
    out.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape of YouTube's automatic captions: each cue repeats the previous line.
    const AUTO: &str = "WEBVTT\nKind: captions\nLanguage: es\n\n\
00:00:00.000 --> 00:00:02.500 align:start position:0%\n\
hola<00:00:00.500><c> a</c><00:00:01.000><c> todos</c>\n\n\
00:00:02.500 --> 00:00:02.510 align:start position:0%\n\
hola a todos\n \n\n\
00:00:02.510 --> 00:00:05.000 align:start position:0%\n\
hola a todos\n\
esto es Oval.\n\n\
00:00:05.000 --> 00:00:07.000\n\
esto es Oval.\n\
&quot;fin&quot; &amp; ya\n";

    #[test]
    fn rolling_captions_are_deduplicated() {
        let lines = parse_vtt(AUTO);
        let text: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text, ["hola a todos", "esto es Oval.", "\"fin\" & ya"]);
        assert_eq!(lines[0].start, 0.0);
        assert_eq!(lines[1].start, 2.51);
    }

    #[test]
    fn two_row_cues_stay_together() {
        let vtt = "WEBVTT

00:00:01.200 --> 00:00:03.360
All right, so here we are
in front of the elephants

00:00:05.000 --> 00:00:06.000
the cool thing
";
        let text: Vec<String> = parse_vtt(vtt).into_iter().map(|l| l.text).collect();
        assert_eq!(
            text,
            [
                "All right, so here we are in front of the elephants",
                "the cool thing"
            ]
        );
    }

    #[test]
    fn formats_render() {
        let lines = parse_vtt(AUTO);
        let meta = Meta {
            title: "Vídeo",
            channel: "Canal",
            url: "https://youtu.be/x",
            note: "Resumen",
            ..Meta::default()
        };
        let md = render(&lines, Format::MdTimes, &meta, 0.0);
        assert!(md.starts_with("# Vídeo\n\nCanal · <https://youtu.be/x>\n\n## TL;DR\n\nResumen\n"));
        assert!(md.contains("[0:00](https://youtu.be/x?t=0s) hola a todos esto es Oval."));
        let srt = render(&lines, Format::Srt, &meta, 0.0);
        assert!(srt.starts_with("1\n00:00:00,000 --> 00:00:02,510\nhola a todos\n"));
        let clipped = clip(
            lines,
            Some(Section {
                start: 3.0,
                end: 6.0,
            }),
        );
        let vtt = render(&clipped, Format::Vtt, &meta, 3.0);
        assert!(vtt.starts_with("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nesto es Oval.\n"));
    }
}
