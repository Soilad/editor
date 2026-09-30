//! Drawing of [`SourcePreview`](crate::thumbnails::SourcePreview) parts inside
//! clips, shared by [`ClipEntry`](super::clip_entry::ClipEntry) and
//! [`ClipTimeline`](super::clip_timeline::ClipTimeline).

use iced::{
    Border, Color, Point, Rectangle, Shadow, Size,
    advanced::{
        image::{self, Image},
        renderer::Quad,
    },
};

use crate::thumbnails::{Frame, Waveform};

/// Width of one waveform bar, and the gap after it.
const BAR_WIDTH: f32 = 2.0;
const BAR_GAP: f32 = 1.0;

/// Draw `frame` at the left of `area`, filling its height. Returns the width
/// it took.
pub fn draw_frame<Renderer>(
    renderer: &mut Renderer,
    frame: &Frame,
    area: Rectangle,
    visible: Rectangle,
) -> f32
where
    Renderer: image::Renderer<Handle = image::Handle>,
{
    let width = (area.height * frame.aspect_ratio()).min(area.width);
    let bounds = Rectangle::new(area.position(), Size::new(width, area.height));
    let Some(clip) = bounds.intersection(&visible) else {
        return width;
    };
    // Scale by height and let the clip cut off whatever spills past `width`.
    let image_bounds = Rectangle::new(
        area.position(),
        Size::new(area.height * frame.aspect_ratio(), area.height),
    );
    renderer.draw_image(Image::new(frame.handle.clone()), image_bounds, clip);
    width
}

/// Draw the part of `waveform` between source times `from` and `to` (in
/// seconds) across `area`, as bars mirrored around its middle.
pub fn draw_waveform<Renderer>(
    renderer: &mut Renderer,
    waveform: &Waveform,
    area: Rectangle,
    visible: Rectangle,
    from: f64,
    to: f64,
    color: Color,
) where
    Renderer: iced::advanced::Renderer,
{
    if area.width <= 0.0 || to <= from {
        return;
    }
    let step = BAR_WIDTH + BAR_GAP;
    let secs_per_pixel = (to - from) / f64::from(area.width);
    // Only the columns inside the visible part of the area.
    let left = (visible.x.max(area.x) - area.x).max(0.0);
    let right = (visible.x + visible.width).min(area.x + area.width) - area.x;
    let mut x = (left / step).floor() * step;
    while x < right {
        let t = from + f64::from(x) * secs_per_pixel;
        let peak = waveform.peak_between(t, t + f64::from(step) * secs_per_pixel);
        let height = (peak * area.height).max(1.0);
        let bar = Rectangle::new(
            Point::new(area.x + x, area.center_y() - height / 2.0),
            Size::new(BAR_WIDTH.min(area.width - x), height),
        );
        if let Some(bar) = bar.intersection(&visible) {
            renderer.fill_quad(
                Quad {
                    bounds: bar,
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                },
                color,
            );
        }
        x += step;
    }
}
