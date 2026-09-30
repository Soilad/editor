use std::time::Duration;

use avio::{ClipId, Timeline, Track, TrackId, TrackKind};
use iced::Color;

pub fn to_iced_color(color: avio::Color) -> Color {
    Color::from_rgba8(color.r, color.g, color.b, color.a as f32 / 255.0)
}

pub fn to_avio_color(color: Color) -> avio::Color {
    let [r, g, b, a] = color.into_rgba8();
    avio::Color::rgba(r, g, b, a)
}

/// Every track on the timeline, video first, then audio.
pub fn tracks(timeline: &Timeline) -> impl Iterator<Item = &Track> {
    timeline
        .video_tracks()
        .iter()
        .chain(timeline.audio_tracks())
}

/// The track shown on timeline row `row`: video tracks top-down from the
/// highest, then audio tracks.
pub fn track_at_row(timeline: &Timeline, row: usize) -> Option<&Track> {
    timeline
        .video_tracks()
        .iter()
        .rev()
        .chain(timeline.audio_tracks())
        .nth(row)
}

/// The clip with this id, with the track holding it.
pub fn find_clip(timeline: &Timeline, id: ClipId) -> Option<(TrackId, &avio::Clip)> {
    tracks(timeline).find_map(|track| {
        let clip = track.clips.iter().find(|clip| clip.id == id)?;
        Some((track.id, clip))
    })
}

/// The track of `kind` that a clip dropped on `dropped` goes to: `dropped`
/// itself when it is of that kind, otherwise the `kind` track at the same
/// position in its list (the last one when there are fewer). `None` when the
/// timeline has no `kind` track.
pub fn matching_track(timeline: &Timeline, kind: TrackKind, dropped: TrackId) -> Option<TrackId> {
    let (same, other) = match kind {
        TrackKind::Video => (timeline.video_tracks(), timeline.audio_tracks()),
        TrackKind::Audio => (timeline.audio_tracks(), timeline.video_tracks()),
    };
    if same.iter().any(|track| track.id == dropped) {
        return Some(dropped);
    }
    let index = other
        .iter()
        .position(|track| track.id == dropped)
        .unwrap_or(0);
    same.get(index).or(same.last()).map(|track| track.id)
}

/// Playback speed of `clip`, kept away from zero.
pub fn clip_speed(clip: &avio::Clip) -> f64 {
    clip.speed.max(0.01)
}

/// Length `clip` takes up on the timeline (source duration divided by speed),
/// if its duration is known.
pub fn footprint(clip: &avio::Clip) -> Option<Duration> {
    Duration::try_from_secs_f64(clip.duration()?.as_secs_f64() / clip_speed(clip)).ok()
}

/// Timeline start and end of `clip`, if its length is known.
pub fn span(clip: &avio::Clip) -> Option<(Duration, Duration)> {
    Some((clip.offset, clip.offset + footprint(clip)?))
}
