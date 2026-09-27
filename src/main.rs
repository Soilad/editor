mod clip_components;
mod widgets;

use std::rc::Rc;

use iced::{
    Element, Length, Point, Theme,
    widget::{
        column, grid,
        pane_grid::{self},
        scrollable, text,
    },
};

use crate::widgets::clip_entry::ClipEntry;
use crate::widgets::clip_timeline::ClipTimeline;
use crate::widgets::timeline::Timeline;
use std::fs;

#[derive(Debug)]
enum PaneType {
    Clips,
    Timeline,
    Viewer,
    Browser,
    ColorGrading,
}

struct App {
    panes: pane_grid::State<PaneType>,
    clips_timelines: Vec<ClipTimeline>,
    clips_entries: Vec<ClipEntry<Message>>,
    dragging_clip_index: Option<usize>,
}

impl Default for App {
    fn default() -> Self {
        let (mut panes, browser) = pane_grid::State::new(PaneType::Browser);
        let (clip, split) = panes
            .split(pane_grid::Axis::Vertical, browser, PaneType::Clips)
            .unwrap();
        panes.resize(split, 0.15);
        let (timeline, _) = panes
            .split(pane_grid::Axis::Horizontal, clip, PaneType::Timeline)
            .unwrap();
        panes.split(pane_grid::Axis::Vertical, clip, PaneType::Viewer);
        panes.split(pane_grid::Axis::Vertical, timeline, PaneType::ColorGrading);
        Self {
            panes,
            clips_timelines: vec![],
            clips_entries: (1..5)
                .map(|x| {
                    ClipEntry::new(x.to_string(), 100.0)
                        .on_press(Message::ClipPressed(x))
                        .on_drag(move |p| Message::ClipMoved(x, p))
                        .on_drop(Message::ClipDropped(x))
                })
                .collect(),
            dragging_clip_index: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Resized(pane_grid::ResizeEvent),
    Dragged(pane_grid::DragEvent),
    MouseReleased,
    ClipPressed(usize),
    ClipMoved(usize, Point),
    /// Carries the index of the clip entry being dropped.
    ClipDropped(usize),
    /// Fired by the Timeline when a dragged clip is released over it.
    /// Carries the drop position and the clip-entry index.
    ClipDroppedToTimeline(Point, usize),
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        pane_grid::PaneGrid::new(&self.panes, |_pane, state, _is_maximized| {
            let (title, content): (&str, Element<'_, Message>) = match state {
                PaneType::Clips => (
                    "Clips",
                    grid(self.clips_entries.iter().cloned().map(Element::from))
                        .height(2000.0)
                        .into(),
                ),
                PaneType::Timeline => ("Timeline", {
                    let dragging_index = self.dragging_clip_index;
                    scrollable(
                        Timeline::with_clips(self.clips_timelines.clone())
                            .dragging_clip(dragging_index.is_some())
                            .on_drop(move |pos| {
                                if let Some(idx) = dragging_index {
                                    Message::ClipDroppedToTimeline(pos, idx)
                                } else {
                                    // Shouldn't happen, but satisfy the type
                                    Message::ClipDroppedToTimeline(pos, usize::MAX)
                                }
                            }),
                    )
                    .direction(scrollable::Direction::Both {
                        vertical: scrollable::Scrollbar::new(),
                        horizontal: scrollable::Scrollbar::new(),
                    })
                    .into()
                }),
                PaneType::Viewer => ("Viewer", text("pipi").into()),
                PaneType::Browser => {
                    let fih: Vec<Element<_>> = fs::read_dir(".")
                        .unwrap()
                        .map(|x| {
                            let name = x.unwrap().file_name();
                            text(name.to_string_lossy().into_owned()).into()
                        })
                        .collect::<Vec<Element<_>>>();
                    ("Browser", column(fih).width(Length::Shrink).into())
                }
                PaneType::ColorGrading => ("ColorGrading", text("ColorGrading").into()),
            };
            pane_grid::Content::new(content).title_bar(pane_grid::TitleBar::new(text(title)))
        })
        .on_resize(10, Message::Resized)
        .on_drag(Message::Dragged)
        .into()
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Resized(pane_grid::ResizeEvent { split, ratio, .. }) => {
                self.panes.resize(split, ratio);
            }
            Message::Dragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.drop(pane, target);
            }
            Message::Dragged(x) => {
                dbg!(x);
            }
            Message::ClipPressed(x) => {
                dbg!(x);
            }
            Message::ClipDropped(index) => {
                // Mark which clip entry is being dragged so the Timeline knows
                self.dragging_clip_index = Some(index);
            }
            Message::ClipMoved(i, p) => {
                self.dragging_clip_index = Some(i);
                if let Some(clip) = self.clips_entries.get_mut(i) {
                    clip.position = p;
                }
            }
            Message::ClipDroppedToTimeline(_pos, index) => {
                if index != usize::MAX {
                    if let Some(entry) = self.clips_entries.get(index) {
                        let new_clip = ClipTimeline::new(
                            entry.name.clone(),
                            entry.length,
                            Rc::clone(&entry.component),
                        );
                        self.clips_timelines.push(new_clip);
                    }
                }
                self.dragging_clip_index = None;
            }
            Message::MouseReleased => {
                self.dragging_clip_index = None;
            }
        }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        iced::event::listen_with(|event, _status, _window_id| match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(Message::MouseReleased)
            }
            _ => None,
        })
    }
}

fn theme(_app: &App) -> Theme {
    Theme::GruvboxDark
}

fn main() -> Result<(), iced::Error> {
    iced::application(App::default, App::update, App::view)
        .theme(theme)
        .subscription(App::subscription)
        .run()
}
