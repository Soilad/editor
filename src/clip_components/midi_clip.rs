//! MIDI clips live in the project's own state ([`OwnState`](crate::project::OwnState)),
//! not in avio, so this component works in ticks and doesn't implement
//! [`ClipComponent`](crate::clip_components::ClipComponent), whose operations
//! are all avio commands.

use std::time::Duration;

use crate::project::{MidiClip, Note, PPQ, Ticks};

#[derive(Debug, Clone)]
pub struct MidiComponent {
    pub clip: MidiClip,
}

impl MidiComponent {
    /// An empty clip at `start`, `length` ticks long.
    pub fn new(start: Ticks, length: Ticks) -> Self {
        Self {
            clip: MidiClip {
                start,
                length,
                notes: Vec::new(),
            },
        }
    }

    pub fn end(&self) -> Ticks {
        self.clip.start + self.clip.length
    }

    /// Wall-clock length at a constant `bpm`.
    pub fn duration(&self, bpm: f64) -> Duration {
        Duration::from_secs_f64(self.clip.length as f64 / PPQ as f64 * 60.0 / bpm.max(1.0))
    }

    /// Adds a note, keeping notes sorted by start. Notes past the clip's end
    /// are kept; they're just not heard until the clip is lengthened.
    pub fn add_note(&mut self, note: Note) {
        let index = self.clip.notes.partition_point(|n| n.start <= note.start);
        self.clip.notes.insert(index, note);
    }

    /// The two halves of a razor cut at timeline position `at`, or `None` when
    /// `at` is not strictly inside the clip. A note crossing the cut is
    /// shortened to end at it; the right half doesn't retrigger it.
    pub fn split(&self, at: Ticks) -> Option<(MidiClip, MidiClip)> {
        if at <= self.clip.start || at >= self.end() {
            return None;
        }
        let cut = at - self.clip.start;
        let (before, after): (Vec<_>, Vec<_>) =
            self.clip.notes.iter().cloned().partition(|n| n.start < cut);
        let left = MidiClip {
            start: self.clip.start,
            length: cut,
            notes: before
                .into_iter()
                .map(|n| Note {
                    length: n.length.min(cut - n.start),
                    ..n
                })
                .collect(),
        };
        let right = MidiClip {
            start: at,
            length: self.clip.length - cut,
            notes: after
                .into_iter()
                .map(|n| Note {
                    start: n.start - cut,
                    ..n
                })
                .collect(),
        };
        Some((left, right))
    }

    /// A detached copy of this clip, to be placed at a new start.
    pub fn copy(&self) -> MidiClip {
        self.clip.clone()
    }
}
