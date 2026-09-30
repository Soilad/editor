//! Overwrite editing: a clip placed on a track takes its span over, trimming,
//! splitting or removing whatever was already there.

use std::time::Duration;

use avio::{ClipId, Command, Timeline};

use crate::helper_funcs::{span, tracks};

/// The clip a placing command puts down, looked up in `timeline` right after the
/// command was applied. `None` for commands that don't place a clip.
pub fn placed_clip(timeline: &Timeline, command: &Command) -> Option<ClipId> {
    match command {
        // avio appends added clips to the end of the track's clip list.
        Command::AddClip { track, .. } => tracks(timeline)
            .find(|t| t.id == *track)
            .and_then(|t| t.clips.last())
            .map(|clip| clip.id),
        Command::MoveClip { clip, .. }
        | Command::MoveClipToTrack { clip, .. }
        | Command::TrimClip { clip, .. } => Some(*clip),
        _ => None,
    }
}

/// Commands that clear the span of clip `id` on its track, so it no longer sits
/// on top of any other clip.
pub fn clear_under(timeline: &Timeline, id: ClipId) -> Vec<Command> {
    let Some((track, placed)) = tracks(timeline)
        .find_map(|t| Some((t, t.clips.iter().find(|clip| clip.id == id)?)))
    else {
        return Vec::new();
    };
    let Some((start, end)) = span(placed) else {
        return Vec::new();
    };

    let mut commands = Vec::new();
    for other in track.clips.iter().filter(|clip| clip.id != id) {
        let Some((other_start, other_end)) = span(other) else {
            continue;
        };
        if other_start >= end || other_end <= start {
            continue;
        }

        let in_point = other.in_point.unwrap_or(Duration::ZERO);
        // Source position of a timeline instant inside `other`.
        let source_at = |at: Duration| in_point + (at - other_start).mul_f64(other.speed);
        let covers_head = start <= other_start;
        let covers_tail = end >= other_end;

        match (covers_head, covers_tail) {
            (true, true) => commands.push(Command::RemoveClip { clip: other.id }),
            // The placed clip lands in the middle: cut out the part under it. The
            // split's left half keeps the id, and is then trimmed back to `start`.
            (false, false) => {
                commands.push(Command::SplitClip {
                    clip: other.id,
                    at: end,
                });
                commands.push(Command::TrimClip {
                    clip: other.id,
                    in_point: other.in_point,
                    out_point: Some(source_at(start)),
                });
            }
            (false, true) => commands.push(Command::TrimClip {
                clip: other.id,
                in_point: other.in_point,
                out_point: Some(source_at(start)),
            }),
            (true, false) => {
                commands.push(Command::TrimClip {
                    clip: other.id,
                    in_point: Some(source_at(end)),
                    out_point: other.out_point,
                });
                commands.push(Command::MoveClip {
                    clip: other.id,
                    offset: end,
                });
            }
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    /// A timeline with one 10s clip at 0s, and `placed` added on top of it.
    /// Returns the timeline after overwriting, plus the placed clip's id.
    fn overwrite(start: f64, end: f64) -> (Timeline, ClipId) {
        let base = avio::Clip::new("a.mp4").trim(secs(0.0), secs(10.0));
        let timeline = Timeline::builder()
            .canvas(1920, 1080)
            .frame_rate(30.0)
            .video_track(vec![base])
            .build()
            .unwrap();
        let track = timeline.video_tracks()[0].id;
        let add = Command::AddClip {
            track,
            clip: Box::new(
                avio::Clip::new("b.mp4")
                    .trim(secs(0.0), secs(end - start))
                    .offset(secs(start)),
            ),
        };
        let mut timeline = avio::apply(&timeline, &add).unwrap();
        let placed = placed_clip(&timeline, &add).unwrap();
        for command in clear_under(&timeline, placed) {
            timeline = avio::apply(&timeline, &command).unwrap();
        }
        (timeline, placed)
    }

    /// `(start, end, in_point)` of every clip except `placed`, sorted by start.
    fn others(timeline: &Timeline, placed: ClipId) -> Vec<(f64, f64, f64)> {
        let mut spans: Vec<_> = timeline.video_tracks()[0]
            .clips
            .iter()
            .filter(|clip| clip.id != placed)
            .map(|clip| {
                let (start, end) = span(clip).unwrap();
                let in_point = clip.in_point.unwrap_or_default();
                (start.as_secs_f64(), end.as_secs_f64(), in_point.as_secs_f64())
            })
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        spans
    }

    fn assert_spans(actual: Vec<(f64, f64, f64)>, expected: &[(f64, f64, f64)]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (a.0 - e.0).abs() < 1e-6 && (a.1 - e.1).abs() < 1e-6 && (a.2 - e.2).abs() < 1e-6,
                "{actual:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn covering_removes() {
        let (timeline, placed) = overwrite(0.0, 12.0);
        assert_spans(others(&timeline, placed), &[]);
    }

    #[test]
    fn middle_splits() {
        let (timeline, placed) = overwrite(3.0, 5.0);
        assert_spans(others(&timeline, placed), &[(0.0, 3.0, 0.0), (5.0, 10.0, 5.0)]);
    }

    #[test]
    fn tail_trims_out_point() {
        let (timeline, placed) = overwrite(7.0, 12.0);
        assert_spans(others(&timeline, placed), &[(0.0, 7.0, 0.0)]);
    }

    #[test]
    fn head_trims_in_point_and_moves() {
        let (timeline, placed) = overwrite(0.0, 4.0);
        assert_spans(others(&timeline, placed), &[(4.0, 10.0, 4.0)]);
    }

    #[test]
    fn untouched_when_apart() {
        let (timeline, placed) = overwrite(10.0, 12.0);
        assert_spans(others(&timeline, placed), &[(0.0, 10.0, 0.0)]);
    }
}
