mod clip_components;
mod widgets;

use iced::{
    Element, Length, Size, Theme, advanced::Widget, widget::{
        column, grid, pane_grid::{self}, scrollable, text
    } 
};

use crate::{clip_components::video_clip::VideoComponent, widgets::clip_timeline::ClipTimeline};
use crate::widgets::clip_entry::ClipEntry;
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

struct App<'a> {
    value: i32,
    panes: pane_grid::State<PaneType>,
    clips: Vec<ClipTimeline<'a>>,
    dragging_clip: Option<ClipTimeline<'a>>,
}

impl Default for App<'_> {
    fn default() -> Self {
        let (mut panes, pane) = pane_grid::State::new(PaneType::Browser);
        let (clip, _) = panes
            .split(pane_grid::Axis::Vertical, pane, PaneType::Clips)
            .unwrap();
        let (timeline, _) = panes
            .split(pane_grid::Axis::Horizontal, clip, PaneType::Timeline)
            .unwrap();
        panes.split(pane_grid::Axis::Vertical, clip, PaneType::Viewer);
        panes.split(pane_grid::Axis::Vertical, timeline, PaneType::ColorGrading);
        Self {
            value: 0,
            panes,
            clips: vec![
                ClipTimeline::new("Opening shot", 210.0, &VideoComponent {}),
                ClipTimeline::new("Interview", 168.0, &VideoComponent {}),
                ClipTimeline::new("Music bed", 300.0, &VideoComponent {}),
            ],
            dragging_clip: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Increment,
    Decrement,
    Resized(pane_grid::ResizeEvent),
    Dragged(pane_grid::DragEvent),
    ClipDragStarted(String, f32),
    ClipDroppedToTimeline,
    MouseReleased,
}

impl App<'_> {
    pub fn view(&self) -> Element<'_, Message> {
        pane_grid::PaneGrid::new(&self.panes, |_pane, state, _is_maximized| {
            let (title, content): (&str, Element<'_, Message>) = match state {
                PaneType::Clips => (
                    "Clips",
                    grid((1..2).map(|_x| {
                        ClipEntry::new(_x.to_string(), 100.0)
                            .on_press(Message::ClipDragStarted(_x.to_string(), 100.0))
                            .into()
                    }))
                    .into(),
                ),
                PaneType::Timeline => (
                    "Timeline",
                    scrollable(
                        Timeline::with_clips(self.clips.clone())
                            .dragging_clip(self.dragging_clip.is_some())
                            .on_drop(Message::ClipDroppedToTimeline),
                    )
                    .direction(
                        scrollable::Direction::Both {
                            vertical: scrollable::Scrollbar::new(),
                            horizontal: scrollable::Scrollbar::new()
                        }
                    )
                    .into()
                ),
                PaneType::Viewer => ("Viewer", text("pipi").into()),
                PaneType::Browser => { 
                    let fih: Vec<Element<_>> = fs::read_dir(".")
                        .unwrap()
                        .map(
                            |x| {
                                let name = x.unwrap().file_name();
                                text(
                                    name
                                    .to_string_lossy()
                                    .into_owned()
                                ).into()
                            }
                        ).collect::<Vec<Element<_>>>();
                    (
                    "Browser",
                    column(
                        fih
                    )
                    .width(Length::Fill)
                    .into(),
                ) },
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
            Message::Increment => {
                self.value += 1;
            }
            Message::Decrement => {
                self.value -= 1;
            }
            Message::Resized(pane_grid::ResizeEvent { split, ratio, .. }) => {
                self.panes.resize(split, ratio);
            }
            Message::Dragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.drop(pane, target);
            }
            Message::Dragged(_) => {}
            Message::ClipDragStarted(name, length) => {
                self.dragging_clip = Some(ClipTimeline::new(name, length, &VideoComponent {}));
            }
            Message::ClipDroppedToTimeline => {
                if let Some(clip) = self.dragging_clip.take() {
                    self.clips.push(clip);
                }
            }
            Message::MouseReleased => {
                self.dragging_clip = None;
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
