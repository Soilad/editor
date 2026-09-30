//! Visual previews of media sources, shown on clips in the clip browser and on
//! the timeline: a video's first frame and an audio stream's waveform.
//!
//! Both are decoded once per source file, off the UI thread, and shared by
//! every clip cut from that file.

use std::{path::PathBuf, sync::Arc, time::Duration};

use iced::advanced::image::Handle;

/// Length of source audio each waveform peak covers.
pub const WAVEFORM_INTERVAL: Duration = Duration::from_millis(20);
/// Height first frames are decoded at; the width follows the aspect ratio.
const FRAME_HEIGHT: u32 = 96;

/// Everything loaded for one source file. A part is `None` when the file has
/// no such stream, or when decoding it failed.
#[derive(Debug, Clone, Default)]
pub struct SourcePreview {
    pub frame: Option<Frame>,
    pub waveform: Option<Waveform>,
}

/// A decoded video frame, ready to draw.
#[derive(Debug, Clone)]
pub struct Frame {
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
}

impl Frame {
    pub fn aspect_ratio(&self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }
}

/// Peak amplitudes of a source's audio, one per [`WAVEFORM_INTERVAL`], as
/// linear values in `0.0..=1.0`.
#[derive(Debug, Clone)]
pub struct Waveform {
    peaks: Arc<[f32]>,
}

impl Waveform {
    /// Length of audio covered, in seconds.
    pub fn duration(&self) -> f64 {
        self.peaks.len() as f64 * WAVEFORM_INTERVAL.as_secs_f64()
    }

    /// Loudest peak between source times `from` and `to`, in seconds.
    pub fn peak_between(&self, from: f64, to: f64) -> f32 {
        let interval = WAVEFORM_INTERVAL.as_secs_f64();
        let first = (from / interval).floor().max(0.0) as usize;
        // Always read at least one peak, so zoomed-in columns aren't blank.
        let last = ((to / interval).ceil() as usize).max(first + 1);
        self.peaks
            .get(first..last.min(self.peaks.len()))
            .map_or(0.0, |peaks| peaks.iter().copied().fold(0.0, f32::max))
    }
}

/// Decodes the previews of the file at `path`: its first frame when `video`
/// is set, its waveform when `audio` is set. Runs on a blocking thread.
pub async fn load(path: PathBuf, video: bool, audio: bool) -> (PathBuf, SourcePreview) {
    let source = path.clone();
    let preview = tokio::task::spawn_blocking(move || SourcePreview {
        frame: video
            .then(|| first_frame(&source))
            .and_then(|frame| frame.inspect_err(|e| eprintln!("first frame: {e}")).ok()),
        waveform: audio
            .then(|| waveform(&source))
            .and_then(|waveform| waveform.inspect_err(|e| eprintln!("waveform: {e}")).ok()),
    })
    .await
    .unwrap_or_default();
    (path, preview)
}

fn first_frame(path: &PathBuf) -> Result<Frame, String> {
    let mut decoder = ff_decode::VideoDecoder::open(path)
        .output_format(avio::PixelFormat::Rgba)
        .output_height(FRAME_HEIGHT)
        .build()
        .map_err(|e| e.to_string())?;
    let frame = decoder
        .decode_one()
        .map_err(|e| e.to_string())?
        .ok_or("the video has no frames")?;

    let (width, height) = (frame.width(), frame.height());
    let data = frame.plane(0).ok_or("decoded frame has no pixels")?;
    let stride = frame.stride(0).ok_or("decoded frame has no stride")?;
    // Rows may be padded past their pixels; the image wants them packed.
    let row = width as usize * 4;
    let pixels: Vec<u8> = data
        .chunks(stride)
        .take(height as usize)
        .flat_map(|line| &line[..row])
        .copied()
        .collect();
    if pixels.len() != row * height as usize {
        return Err("decoded frame is smaller than its size".to_owned());
    }
    Ok(Frame {
        handle: Handle::from_rgba(width, height, pixels),
        width,
        height,
    })
}

fn waveform(path: &PathBuf) -> Result<Waveform, String> {
    let samples = avio::WaveformAnalyzer::new(path)
        .interval(WAVEFORM_INTERVAL)
        .run()
        .map_err(|e| e.to_string())?;
    let peaks = samples
        .iter()
        // dBFS to linear amplitude; silence (-inf dB) becomes 0.
        .map(|sample| 10f32.powf(sample.peak_db / 20.0).clamp(0.0, 1.0))
        .collect();
    Ok(Waveform { peaks })
}
