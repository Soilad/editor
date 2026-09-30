use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ff_preview::PlayerEvent;

use crate::helper_funcs::{footprint, tracks};

/// A running avio player whose frames land in a shared RGBA slot.
#[derive(Clone)]
pub struct Preview {
    handle: avio::PlayerHandle,
    pub frames: Arc<Mutex<Option<avio::RgbaFrame>>>, // latest composited frame
    mode: Arc<Mutex<Mode>>,
    /// Cleared once the player thread exits (end of timeline, stop, or a failed
    /// seek); the player can't be restarted after that, only reopened.
    running: Arc<AtomicBool>,
    /// Keeps the audio output playing; it closes once every clone is dropped.
    _audio: mpsc::Sender<()>,
}

/// What the sink does with the frames the runner hands it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// Keep every frame.
    Playing,
    /// Drop frames until the seek to this position lands, keep the next one, then pause.
    Still(Duration),
    /// The seek landed; the next frame is the one to keep.
    Landed,
    /// Paused: drop the frame or two the runner presents before it sees the pause.
    Held,
}

impl Preview {
    /// Opens a player for `timeline` positioned at `start`, playing or paused,
    /// and runs it on its own thread.
    ///
    /// Blocks while the sources are opened, so call it off the UI thread.
    pub fn open(timeline: &avio::Timeline, start: Duration, play: bool) -> Result<Self, String> {
        let (mut runner, handle) =
            ff_preview::ScenePlayer::open(&stacked_scene(timeline)?).map_err(|e| e.to_string())?;
        if let Some(gpu) = avio::GpuPreviewCompositor::new() {
            runner.set_gpu_compositor(Box::new(gpu));
        }

        let frames = Arc::new(Mutex::new(None));
        let mode = Arc::new(Mutex::new(Mode::Held));
        runner.set_sink(Box::new(Sink {
            frames: Arc::clone(&frames),
            mode: Arc::clone(&mode),
            handle: handle.clone(),
        })); // must happen before run()

        let running = Arc::new(AtomicBool::new(true));
        let preview = Self {
            _audio: play_audio(handle.clone()),
            handle,
            frames,
            mode,
            running,
        };
        // Queued before run() starts, so the first thing the runner does is jump
        // to `start`.
        if start >= timeline_end(timeline) {
            preview.handle.pause();
        } else if play {
            preview.play_from(start);
        } else {
            preview.show(start);
        }

        let flag = Arc::clone(&preview.running);
        // run() blocks until playback ends or handle.stop()
        std::thread::spawn(move || {
            if let Err(e) = runner.run() {
                eprintln!("preview: {e}");
            }
            flag.store(false, Ordering::Release);
        });
        Ok(preview)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn play(&self) {
        *self.mode.lock().unwrap() = Mode::Playing;
        self.handle.play();
    }

    fn play_from(&self, at: Duration) {
        self.handle.seek(at);
        self.play();
    }

    pub fn pause(&self) {
        *self.mode.lock().unwrap() = Mode::Held;
        self.handle.pause();
    }

    pub fn stop(&self) {
        self.handle.stop();
    }

    /// Moves to `at`, carrying on playing if `playing`, else presenting the one
    /// composited frame there.
    pub fn seek(&self, at: Duration, playing: bool) {
        if playing {
            self.handle.seek(at);
        } else {
            self.show(at);
        }
    }

    /// Presents the fully composited frame at `at` and pauses there.
    ///
    /// A seek while paused makes the runner push the base track's raw frame,
    /// with no overlays, so instead play just long enough for one real frame.
    /// Play goes first: it clears the runner's paused flag at once, so by the
    /// time the seek is applied the raw path is already off.
    fn show(&self, at: Duration) {
        *self.mode.lock().unwrap() = Mode::Still(at);
        self.handle.play();
        self.handle.seek(at);
    }
}

/// Sample rate of the player's mixed audio: interleaved stereo `f32`.
const SAMPLE_RATE: u32 = 48_000;

/// Plays `handle`'s mixed audio on the default output device until the
/// returned sender and all its clones are dropped.
///
/// The player only mixes audio; something has to pull the samples out and
/// hand them to a device. The stream lives on its own thread since it can't
/// move between threads on every platform.
fn play_audio(handle: avio::PlayerHandle) -> mpsc::Sender<()> {
    let (keep_alive, closed) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let stream = match open_output(handle) {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("preview audio: {e}");
                return;
            }
        };
        // Nothing is ever sent; this returns once every sender is gone.
        let _ = closed.recv();
        drop(stream);
    });
    keep_alive
}

fn open_output(handle: avio::PlayerHandle) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no audio output device")?;
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };
    let stream = device
        .build_output_stream(
            &config,
            move |out: &mut [f32], _| {
                // Empty while paused or stopped, short on underrun: pad with silence.
                let samples = handle.pop_audio_samples(out.len());
                let n = samples.len().min(out.len());
                out[..n].copy_from_slice(&samples[..n]);
                out[n..].fill(0.0);
            },
            |e| eprintln!("preview audio: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Receives frames on the runner's thread.
struct Sink {
    frames: Arc<Mutex<Option<avio::RgbaFrame>>>,
    mode: Arc<Mutex<Mode>>,
    handle: avio::PlayerHandle,
}

impl avio::FrameSink for Sink {
    fn push_frame(&mut self, rgba: &[u8], width: u32, height: u32, pts: Duration) {
        let mut mode = self.mode.lock().unwrap();
        // The runner sends SeekCompleted right before presenting from the new
        // position, on this same thread, so it marks where the wanted frame
        // starts. Drained every frame so the bounded event queue never fills
        // and drops it.
        while let Some(event) = self.handle.poll_event() {
            if let PlayerEvent::SeekCompleted(at) = event
                && *mode == Mode::Still(at)
            {
                *mode = Mode::Landed;
            }
        }
        match *mode {
            Mode::Playing => {}
            Mode::Landed => {
                *mode = Mode::Held;
                self.handle.pause();
            }
            Mode::Still(_) | Mode::Held => return,
        }
        *self.frames.lock().unwrap() = Some(avio::RgbaFrame {
            data: rgba.to_vec(),
            width,
            height,
            pts,
        });
    }
}

/// The timeline as the runner should draw it, with video tracks stacked
/// bottom-up (track 0 lowest).
///
/// The runner treats track 0 as a base that everything hangs off: it won't open
/// without clips there, stops when it ends, and drops seeks in its gaps. So slot
/// a black backdrop spanning the whole timeline in underneath, making every real
/// track an overlay that goes through the compositor the same way.
fn stacked_scene(timeline: &avio::Timeline) -> Result<avio::Scene, String> {
    let backdrop = avio::Timeline::builder()
        .canvas(timeline.canvas_width(), timeline.canvas_height())
        .frame_rate(timeline.frame_rate())
        .video_track(vec![
            avio::Clip::solid(avio::Color::BLACK).trim(Duration::ZERO, timeline_end(timeline)),
        ])
        .build()
        .map_err(|e| e.to_string())?
        .to_scene()
        .video_tracks
        .remove(0);
    let mut scene = timeline.to_scene();
    scene.video_tracks.insert(0, backdrop);
    Ok(scene)
}

/// End of the last clip, video or audio. The black backdrop spans up to here,
/// and the player can't seek at or past it.
pub fn timeline_end(timeline: &avio::Timeline) -> Duration {
    tracks(timeline)
        .flat_map(|track| &track.clips)
        .filter_map(|clip| Some(clip.offset + footprint(clip)?))
        .max()
        .unwrap_or(Duration::ZERO)
}

/// Whether some video clip is on screen at `at`.
pub fn is_covered(timeline: &avio::Timeline, at: Duration) -> bool {
    timeline
        .video_tracks()
        .iter()
        .flat_map(|track| &track.clips)
        .any(|clip| {
            footprint(clip).is_some_and(|length| at >= clip.offset && at < clip.offset + length)
        })
}




