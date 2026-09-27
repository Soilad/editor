use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Shadow, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse,
        renderer::Quad,
        text::{self, Text},
        widget::{Tree, tree},
    },
    alignment, keyboard,
};

use crate::widgets::clip_timeline::{self, ClipTimeline, Hit};

const LABEL_WIDTH: f32 = 132.0;
const TRACK_HEIGHT: f32 = 68.0;
const TIMELINE_WIDTH: f32 = 760.0;
const CLIP_INSET: f32 = 8.0;
const SNAP: f32 = 12.0;
const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 3.0;
const BASE_CONTENT_WIDTH: f32 = 1_800.0;
const SCROLLBAR_HEIGHT: f32 = 14.0;

#[derive(Debug, Clone)]
struct Placement {
    track: usize,
    start: f32,
    length: f32,
}

#[derive(Debug, Default)]
struct State {
    placements: Vec<Placement>,
    /// Set when a clip is dropped onto the timeline; consumed by the next `diff()` call
    /// to position the newly-added clip at the cursor location.
    pending_placement: Option<(f32, usize)>, // (start_x in timeline coords, track index)
    drag: Option<Drag>,
    playhead: f32,
    scroll_x: f32,
    zoom: f32,
    modifiers: keyboard::Modifiers,
}

#[derive(Debug)]
struct Drag {
    mode: DragMode,
}

#[derive(Debug)]
enum DragMode {
    Move { clip: usize, grab_x: f32 },
    ResizeStart { clip: usize, original_end: f32 },
    ResizeEnd { clip: usize },
    Playhead,
    // Scrollbar { grab_x: f32 },
}

/// A compact editor-style timeline with track labels, movable clips, resize handles,
/// a draggable playhead, horizontal scrolling, and cursor-centered zooming.
pub struct Timeline<Message, Renderer> {
    clips: Vec<ClipTimeline>,
    track_labels: Vec<String>,
    on_drop: Option<Box<dyn Fn(Point) -> Message>>,
    is_dragging_clip: bool,
    length: i32,
    _phantom: std::marker::PhantomData<(Message, Renderer)>,
}

impl<Message, Renderer> Timeline<Message, Renderer> {
    pub fn with_clips(clips: Vec<ClipTimeline>) -> Self {
        let track_labels = (1..=clips.len())
            .map(|index| format!("Video {index}"))
            .collect();
        Self {
            clips,
            track_labels,
            on_drop: None,
            is_dragging_clip: false,
            length: 1000,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn on_drop(mut self, f: impl Fn(Point) -> Message + 'static) -> Self {
        self.on_drop = Some(Box::new(f));
        self
    }

    pub fn dragging_clip(mut self, is_dragging: bool) -> Self {
        self.is_dragging_clip = is_dragging;
        self
    }

    fn track_count(&self) -> usize {
        self.track_labels.len().max(self.clips.len()).max(1)
    }

    fn lane_width(&self) -> f32 {
        TIMELINE_WIDTH - LABEL_WIDTH
    }


    fn content_width(&self, state: &State) -> f32 {
        state
            .placements
            .iter()
            .map(|placement| placement.start + placement.length + 360.0)
            .fold(BASE_CONTENT_WIDTH, f32::max)
            .max(self.lane_width() / state.zoom.max(MIN_ZOOM))
    }

    fn max_scroll(&self, state: &State) -> f32 {
        (self.content_width(state) - self.lane_width() / state.zoom.max(MIN_ZOOM)).max(0.0)
    }

    fn clamp_view(&self, state: &mut State) {
        state.zoom = state.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        state.scroll_x = state.scroll_x.clamp(0.0, self.max_scroll(state));
        state.playhead = state.playhead.clamp(0.0, self.content_width(state));
    }

    fn initial_state(&self) -> State {
        State {
            placements: self
                .clips
                .iter()
                .enumerate()
                .map(|(index, clip)| Placement {
                    track: index.min(self.track_count() - 1),
                    start: 24.0 + index as f32 * 84.0,
                    length: clip.length.max(clip_timeline::MIN_LENGTH),
                })
                .collect(),
            zoom: 1.0,
            ..State::default()
        }
    }

    fn lane_bounds(&self, layout: Layout<'_>) -> Rectangle {
        let bounds = layout.bounds();
        Rectangle::new(
            Point::new(bounds.x + LABEL_WIDTH, bounds.y),
            Size::new(self.lane_width(), self.track_count() as f32 * TRACK_HEIGHT),
        )
    }

    fn timeline_to_screen_x(&self, layout: Layout<'_>, state: &State, x: f32) -> f32 {
        layout.bounds().x + LABEL_WIDTH + (x - state.scroll_x) * state.zoom
    }

    fn screen_to_timeline_x(&self, layout: Layout<'_>, state: &State, x: f32) -> f32 {
        state.scroll_x + (x - layout.bounds().x - LABEL_WIDTH) / state.zoom
    }

    fn clip_bounds(&self, layout: Layout<'_>, state: &State, placement: &Placement) -> Rectangle {
        Rectangle::new(
            Point::new(
                self.timeline_to_screen_x(layout, state, placement.start),
                layout.bounds().y + placement.track as f32 * TRACK_HEIGHT + CLIP_INSET,
            ),
            Size::new(
                placement.length * state.zoom,
                TRACK_HEIGHT - CLIP_INSET * 2.0,
            ),
        )
    }

    fn playhead_hit_bounds(&self, layout: Layout<'_>, state: &State) -> Rectangle {
        let x = self.timeline_to_screen_x(layout, state, state.playhead);
        Rectangle::new(
            Point::new(x - 5.0, layout.bounds().y),
            Size::new(10.0, self.track_count() as f32 * TRACK_HEIGHT),
        )
    }

    fn snap(value: f32) -> f32 {
        (value / SNAP).round() * SNAP
    }
}

impl<Message, Renderer> Default for Timeline<Message, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn default() -> Self {
        Self::with_clips(Vec::new())
    }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for Timeline<Message, Renderer>
where
    Renderer: iced::advanced::Renderer + text::Renderer,
{
    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(
            Length::Fill,
            Length::Fill,
            // Length::Fixed(self.track_count() as f32 * TRACK_HEIGHT + SCROLLBAR_HEIGHT),
        )
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(self.initial_state())
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.placements.len() < self.clips.len() {
            let pending = state.pending_placement.take();
            state
                .placements
                .extend(
                    (state.placements.len()..self.clips.len()).map(|index| {
                        let (start, track) = if index == self.clips.len() - 1 {
                            // Last (newly added) clip — use the drop position if available.
                            pending.unwrap_or_else(|| (
                                24.0 + index as f32 * 84.0,
                                index.min(self.track_count() - 1),
                            ))
                        } else {
                            (
                                24.0 + index as f32 * 84.0,
                                index.min(self.track_count() - 1),
                            )
                        };
                        Placement {
                            track,
                            start,
                            length: self.clips[index].length.max(clip_timeline::MIN_LENGTH),
                        }
                    }),
                );
        } else {
            state.placements.truncate(self.clips.len());
        }
        self.clamp_view(state);
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(
            Size::new(
                TIMELINE_WIDTH,
                self.track_count() as f32 * TRACK_HEIGHT + SCROLLBAR_HEIGHT,
            )
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let palette = theme.palette();
        let state = tree.state.downcast_ref::<State>();
        let lane_bounds = self.lane_bounds(layout);
        // let scrollbar_bounds = self.scrollbar_bounds(layout);

        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            },
            palette.background,
        );

        let first_tick = (state.scroll_x / SNAP).floor() as i32 - 1;
        let last_tick =
            ((state.scroll_x + self.lane_width() / state.zoom) / SNAP).ceil() as i32 + 1;
        for tick in first_tick..=last_tick {
            let timeline_x = tick as f32 * SNAP;
            let x = self.timeline_to_screen_x(layout, state, timeline_x);
            if x < lane_bounds.x || x > lane_bounds.x + lane_bounds.width {
                continue;
            }
            let major = tick % 5 == 0;
            let height = if major { 12.0 } else { 6.0 };
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(x, bounds.y),
                        Size::new(1.0, self.track_count() as f32 * TRACK_HEIGHT),
                    ),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                },
                Color {
                    a: if major { 0.25 } else { 0.12 },
                    ..palette.text
                },
            );
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(Point::new(x, bounds.y), Size::new(1.0, height)),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                },
                palette.primary,
            );
        }

        for (clip_timeline, placement) in self.clips.iter().zip(&state.placements) {
            let clip_bounds = self.clip_bounds(layout, state, placement);
            if clip_bounds.x > lane_bounds.x + lane_bounds.width
                || clip_bounds.x + clip_bounds.width < lane_bounds.x
            {
                continue;
            }

            clip_timeline.draw_at(
                renderer,
                theme,
                clip_bounds,
                viewport,
            );
        }


        let playhead_x = self.timeline_to_screen_x(layout, state, state.playhead);
        if playhead_x >= lane_bounds.x && playhead_x <= lane_bounds.x + lane_bounds.width {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(playhead_x - 1.0, bounds.y),
                        Size::new(2.0, self.track_count() as f32 * TRACK_HEIGHT),
                    ),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                },
                palette.danger,
            );
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(playhead_x - 6.0, bounds.y),
                        Size::new(12.0, 8.0),
                    ),
                    border: Border {
                        color: palette.danger,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                palette.danger,
            );
        }

        for track in 0..self.track_count() {
            let y = bounds.y + track as f32 * TRACK_HEIGHT;
            let label_bounds = Rectangle::new(
                Point::new(bounds.x, y),
                Size::new(LABEL_WIDTH, TRACK_HEIGHT),
            );
            let track_lane_bounds = Rectangle::new(
                Point::new(bounds.x + LABEL_WIDTH, y),
                Size::new(self.lane_width(), TRACK_HEIGHT),
            );
            renderer.fill_quad(
                Quad {
                    bounds: label_bounds,
                    border: Border {
                        color: palette.primary,
                        width: 0.5,
                        radius: 0.0.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                palette.background,
            );
            renderer.fill_quad(
                Quad {
                    bounds: track_lane_bounds,
                    border: Border {
                        color: Color {
                            a: 0.20,
                            ..palette.primary
                        },
                        width: 0.5,
                        radius: 0.0.into(),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                Color {
                    a: 0.16,
                    ..palette.text
                },
            );

            let label = self
                .track_labels
                .get(track)
                .map(String::as_str)
                .unwrap_or("Track");
            renderer.fill_text(
                Text {
                    content: label.to_owned(),
                    size: 15.0.into(),
                    line_height: text::LineHeight::default(),
                    font: renderer.default_font(),
                    bounds: Size::new(LABEL_WIDTH - 24.0, 20.0),
                    align_x: text::Alignment::Left,
                    align_y: alignment::Vertical::Center,
                    shaping: text::Shaping::Basic,
                    wrapping: text::Wrapping::None,
                },
                Point::new(bounds.x + 16.0, y + TRACK_HEIGHT / 2.0),
                palette.text,
                *viewport,
            );
        }

    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(position) = cursor.position() else {
                    return;
                };

                if self.playhead_hit_bounds(layout, state).contains(position) {
                    state.drag = Some(Drag {
                        mode: DragMode::Playhead,
                    });
                    shell.request_redraw();
                    return;
                }

                for index in (0..self.clips.len()).rev() {
                    let clip_bounds = self.clip_bounds(layout, state, &state.placements[index]);
                    match ClipTimeline::hit_test(clip_bounds, position) {
                        Some(Hit::StartHandle) => {
                            let placement = &state.placements[index];
                            state.drag = Some(Drag {
                                mode: DragMode::ResizeStart {
                                    clip: index,
                                    original_end: placement.start + placement.length,
                                },
                            });
                            shell.request_redraw();
                            return;
                        }
                        Some(Hit::EndHandle) => {
                            state.drag = Some(Drag {
                                mode: DragMode::ResizeEnd { clip: index },
                            });
                            shell.request_redraw();
                            return;
                        }
                        Some(Hit::Body) => {
                            state.drag = Some(Drag {
                                mode: DragMode::Move {
                                    clip: index,
                                    grab_x: (position.x - clip_bounds.x) / state.zoom,
                                },
                            });
                            shell.request_redraw();
                            return;
                        }
                        None => {}
                    }
                }

                if self.lane_bounds(layout).contains(position) {
                    state.drag = Some(Drag {
                        mode: DragMode::Playhead,
                    });
                    Self::snap(self.screen_to_timeline_x(layout, state, position.x))
                        .clamp(0.0, self.content_width(state));
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(position) = cursor.position() else {
                    return;
                };
                let Some(drag) = state.drag.take() else {
                    return;
                };

                match drag.mode {
                    DragMode::Move { clip, grab_x } => {
                        let max_start =
                            (self.content_width(state) - state.placements[clip].length).max(0.0);
                        let raw_start =
                            self.screen_to_timeline_x(layout, state, position.x) - grab_x;
                        let placement = &mut state.placements[clip];
                        placement.start =
                            Self::snap(raw_start.clamp(0.0, max_start)).min(max_start);
                        placement.track = (((position.y - layout.bounds().y) / TRACK_HEIGHT).floor()
                            as isize)
                            .clamp(0, self.track_count() as isize - 1)
                            as usize;
                        state.drag = Some(Drag {
                            mode: DragMode::Move { clip, grab_x },
                        });
                    }
                    DragMode::ResizeStart { clip, original_end } => {
                        let max_start = (original_end - clip_timeline::MIN_LENGTH).max(0.0);
                        let raw_start = self.screen_to_timeline_x(layout, state, position.x);
                        let snapped_start =
                            Self::snap(raw_start.clamp(0.0, max_start)).min(max_start);
                        let placement = &mut state.placements[clip];
                        ClipTimeline::resize_from_start(
                            &mut placement.start,
                            &mut placement.length,
                            snapped_start,
                        );
                        state.drag = Some(Drag {
                            mode: DragMode::ResizeStart { clip, original_end },
                        });
                    }
                    DragMode::ResizeEnd { clip } => {
                        let start = state.placements[clip].start;
                        let raw_end = self.screen_to_timeline_x(layout, state, position.x);
                        let max_end = self.content_width(state);
                        let snapped_end =
                            Self::snap(raw_end.clamp(start + clip_timeline::MIN_LENGTH, max_end))
                                .min(max_end);
                        ClipTimeline::resize_from_end(
                            start,
                            &mut state.placements[clip].length,
                            snapped_end,
                        );
                        state.drag = Some(Drag {
                            mode: DragMode::ResizeEnd { clip },
                        });
                    }
                    DragMode::Playhead => {
                        state.playhead =
                            Self::snap(self.screen_to_timeline_x(layout, state, position.x))
                                .clamp(0.0, self.content_width(state));
                        state.drag = Some(Drag {
                            mode: DragMode::Playhead,
                        });
                    }
                }
                self.clamp_view(state);
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.take().is_some() {
                    shell.request_redraw();
                }

                if self.is_dragging_clip {
                    if let Some(position) = cursor.position() {
                        if self.lane_bounds(layout).contains(position)
                            || self
                                .lane_bounds(layout)
                                .contains(Point::new(position.x, layout.bounds().y))
                        {
                            // Compute where on the timeline the clip landed.
                            let raw_start = self.screen_to_timeline_x(layout, state, position.x);
                            let start = Self::snap(raw_start).max(0.0);
                            let track = (((position.y - layout.bounds().y) / TRACK_HEIGHT)
                                .floor() as isize)
                                .clamp(0, self.track_count() as isize - 1)
                                as usize;
                            state.pending_placement = Some((start, track));

                            if let Some(on_drop) = &self.on_drop {
                                shell.publish(on_drop(position));
                            }
                        }
                    }
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let Some(position) = cursor.position() else {
                    return;
                };
                if !self.lane_bounds(layout).contains(position)
                    // && !self.scrollbar_bounds(layout).contains(position)
                {
                    return;
                }

                let (x, y) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (*x * 48.0, *y * 48.0),
                    mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
                };

                if state.modifiers.command() {
                    let anchor = self.screen_to_timeline_x(layout, state, position.x);
                    let old_zoom = state.zoom;
                    state.zoom = (state.zoom * (1.0 + y * 0.01)).clamp(MIN_ZOOM, MAX_ZOOM);
                    let screen_offset = (position.x - layout.bounds().x - LABEL_WIDTH) / old_zoom;
                    state.scroll_x = anchor - screen_offset * (old_zoom / state.zoom);
                } else {
                    state.scroll_x += x - y;
                }

                self.clamp_view(state);
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if let Some(drag) = &state.drag {
            return match drag.mode {
                DragMode::ResizeStart { .. } | DragMode::ResizeEnd { .. } => {
                    mouse::Interaction::ResizingHorizontally
                }
                DragMode::Playhead | DragMode::Move { .. } => {
                    mouse::Interaction::Grabbing
                }
            };
        }

        let Some(position) = cursor.position() else {
            return mouse::Interaction::None;
        };

        // if self
        //     .scrollbar_thumb_bounds(layout, state)
        //     .contains(position)
        //     || self.playhead_hit_bounds(layout, state).contains(position)
        // {
        //     return mouse::Interaction::Grab;
        // }

        for index in (0..self.clips.len()).rev() {
            let clip_bounds = self.clip_bounds(layout, state, &state.placements[index]);
            let interaction = ClipTimeline::mouse_interaction_at(
                clip_bounds,
                cursor,
            );
            if interaction != mouse::Interaction::None {
                return interaction;
            }
        }

        mouse::Interaction::None
    }
}

impl<'a, Message: 'a, Renderer> From<Timeline<Message, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer + text::Renderer + 'a,
    Message: 'a,
{
    fn from(widget: Timeline<Message, Renderer>) -> Self {
        Self::new(widget)
    }
}
