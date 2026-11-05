use termune::Result;

#[derive(Clone, Default)]
pub struct Media {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub lyrics: Vec<String>,
    pub times: Vec<Option<f64>>,
    pub lyric_error: bool,
    pub loading: bool,
}
pub struct Download {
    pub lyrics: Result<LyricData>,
}
#[derive(Default)]
pub struct LyricData {
    pub lines: Vec<String>,
    pub times: Vec<Option<f64>>,
}

pub fn active_line(times: &[Option<f64>], position: f64) -> Option<usize> {
    times
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.filter(|t| *t <= position).map(|_| i))
        .next_back()
}
pub fn parse_lyrics(content: &str) -> LyricData {
    let mut rows: Vec<(Option<f64>, String)> = Vec::new();
    let mut offset = 0.0;
    for raw in content.lines().take(10000) {
        let mut text = raw.trim().trim_start_matches('\u{feff}');
        let mut stamps = Vec::new();
        let mut metadata = false;
        while let Some(rest) = text.strip_prefix('[') {
            let Some(end) = rest.find(']') else {
                break;
            };
            let tag = &rest[..end];
            if let Some(value) = tag.strip_prefix("offset:") {
                if let Ok(ms) = value.parse::<f64>()
                    && ms.is_finite()
                {
                    offset = ms / 1000.0;
                }
                metadata = true;
            } else if let Some((minutes, seconds)) = tag.split_once(':') {
                if let (Ok(m), Ok(s)) = (minutes.parse::<u32>(), seconds.parse::<f64>()) {
                    if s.is_finite() && (0.0..60.0).contains(&s) {
                        stamps.push(m as f64 * 60.0 + s);
                    }
                } else {
                    metadata = true;
                }
            } else {
                metadata = true;
            }
            text = rest[end + 1..].trim_start();
        }
        let text = crate::app::clean(text);
        if text.trim().is_empty() || metadata {
            continue;
        }
        if stamps.is_empty() {
            rows.push((None, text));
        } else {
            for stamp in stamps {
                rows.push((Some(stamp), text.clone()));
            }
        }
    }
    if rows.iter().all(|(t, _)| t.is_some()) {
        rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    }
    let (times, lines) = rows
        .into_iter()
        .map(|(time, text)| (time.map(|t| (t - offset).max(0.0)), text))
        .unzip();
    LyricData { lines, times }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lrc_timestamps_offsets_and_plain_text() {
        let l = parse_lyrics("[offset:500]\n[ar:a]\n[00:01.20][00:02.30]一句\n[00:03.40]下一句");
        assert_eq!(l.lines, ["一句", "一句", "下一句"]);
        assert!((l.times[0].unwrap() - 0.7).abs() < 0.001);
        assert_eq!(parse_lyrics("纯文本").times, [None]);
        let times = vec![Some(1.0), Some(3.0), Some(6.0)];
        assert_eq!(active_line(&times, 0.0), None);
        assert_eq!(active_line(&times, 7.0), Some(2));
        assert_eq!(active_line(&times, 2.0), Some(0));
    }
}
