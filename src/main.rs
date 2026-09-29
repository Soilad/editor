mod clip_components;
mod widgets;

use std::{
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

use iced::{
    Color, Element, Length, Point, Renderer, Theme,
    advanced::image,
    theme::Palette,
    widget::{
        button, column, container, grid,
        pane_grid::{self},
        scrollable, text,
    },
};
use iced_aw::{Menu, MenuBar, menu_items};

use crate::widgets::clip_entry::ClipEntry;
use crate::widgets::clip_timeline::ClipTimeline;
use crate::widgets::timeline::Timeline;

use avio::{ClipSource, Editor, MediaInfo, open};
use std::fs;

fn browse(dir: &str) -> Vec<(String, MediaInfo)> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| {
            let path: PathBuf = entry.ok()?.path();
            let info = open(&path).ok()?; // reads container/stream metadata, no decode
            Some((path.to_string_lossy().into_owned(), info))
        })
        .collect()
}

fn open_preview(timeline: avio::Timeline) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let (mut runner, handle) =
                    avio::TimelinePlayer::open(&timeline).map_err(|e| e.to_string())?;

                let sink = avio::RgbaSink::new();
                let frames = sink.frame_handle();
                runner.set_sink(Box::new(sink)); // must happen before run()

                // run() blocks until playback ends or handle.stop()
                tokio::task::spawn_blocking(move || {
                    let _ = runner.run();
                });

                Ok(Preview { handle, frames })
            })
            .await
            .map_err(|e| e.to_string())?
        },
        Message::PreviewReady,
    )
}

#[derive(Clone)]
pub struct Preview {
    handle: avio::PlayerHandle, // cloneable play/pause/seek control
    frames: Arc<Mutex<Option<avio::RgbaFrame>>>, // latest decoded frame
}

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
    editor: Editor,
    clips_entries: Vec<ClipEntry<Message>>,
    dragging_clip_index: Option<usize>,
    preview: Option<Preview>,
    playing: bool,
    frame: Option<image::Handle>,
    last_pts: Option<std::time::Duration>, // adjust to whatever type RgbaFrame::pts is
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

        let timeline = avio::Timeline::builder()
            .canvas(1920, 1080)
            .frame_rate(30.0)
            .video_track(vec![]) // one empty video track to start
            .build()
            .expect("valid timeline");

        Self {
            panes,
            clips_timelines: vec![],
            clips_entries: browse("./src/media/")
                .into_iter()
                .enumerate()
                .map(|(i, (name, info))| {
                    ClipEntry::new(
                        name,
                        info.duration().as_secs_f32(),
                        info.path().to_path_buf(),
                    )
                    .on_press(Message::ClipPressed(i))
                    .on_drag(move |p| Message::ClipMoved(i, p))
                    .on_drop(Message::ClipDropped(i))
                })
                .collect(),
            dragging_clip_index: None,
            editor: Editor::new(timeline),
            frame: None,
            last_pts: None,
            playing: false,
            preview: None,
        }
    }
}

#[derive(Clone)]
pub enum Message {
    NoOp,
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
    PreviewReady(Result<Preview, String>),
    Tick,
    TogglePlay,
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let file_menu: Menu<'_, Message, Theme, Renderer> = Menu::new(menu_items!(
            (button("New").on_press(Message::NoOp).width(Length::Fill)),
            (button("Open").on_press(Message::NoOp).width(Length::Fill)),
            (button("Exit").on_press(Message::NoOp).width(Length::Fill)),
        ))
        .width(100);

        let edit_menu: Menu<'_, Message, _, _> = Menu::new(menu_items!(
            (button("Copy").on_press(Message::NoOp).width(Length::Fill)),
            (button("Paste").on_press(Message::NoOp).width(Length::Fill)),
        ))
        .width(100);

        let head: MenuBar<'_, Message, _, _> = MenuBar::new(menu_items!(
            (container(text("File")), file_menu),
            (container(text("Edit")), edit_menu),
        ))
        .spacing(10);

        let body = pane_grid::PaneGrid::new(&self.panes, |_pane, state, _is_maximized| {
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
                PaneType::Viewer => (
                    "Viewer",
                    match &self.frame {
                        Some(h) => iced::widget::image(h.clone())
                            .content_fit(iced::ContentFit::Contain)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .into(),
                        None => text("no preview").into(),
                    },
                ),
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
        .on_drag(Message::Dragged);
        column![head, body].into()
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::NoOp => {
                todo!()
            }
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
                    dbg!(_pos);
                    if let Some(entry) = self.clips_entries.get(index) {
                        if let Some(component) = &entry.component {
                            let new_clip = ClipTimeline::new(
                                entry.name.clone(),
                                entry.length,
                                Rc::clone(component),
                            );
                            self.clips_timelines.push(new_clip);
                        }
                        if let Some(avio_component) = &entry.avio_component {
                            let track = self.editor.current().video_tracks()[0].id;
                            self.editor.apply(&avio::Command::AddClip {
                                track: track,
                                clip: Box::new(avio_component.clone()),
                            });
                        }
                    }
                }
                self.dragging_clip_index = None;
            }
            Message::MouseReleased => {
                self.dragging_clip_index = None;
            }
            Message::PreviewReady(Ok(p)) => {
                self.preview = Some(p);
            }
            Message::PreviewReady(Err(e)) => eprintln!("preview: {e}"),
            Message::TogglePlay => {
                if let Some(p) = &self.preview {
                    if self.playing {
                        p.handle.pause()
                    } else {
                        p.handle.play()
                    }
                    self.playing = !self.playing;
                }
            }
            Message::Tick => {
                if let Some(p) = &self.preview {
                    if let Some(f) = p.frames.lock().unwrap().as_ref() {
                        if self.last_pts != Some(f.pts) {
                            self.last_pts = Some(f.pts);
                            self.frame =
                                Some(image::Handle::from_rgba(f.width, f.height, f.data.clone()));
                        }
                    }
                }
            }
        }
        iced::Task::none()
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let mouse = iced::event::listen_with(|event, _status, _window_id| match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(Message::MouseReleased)
            }
            _ => None,
        });
        let tick = if self.preview.is_some() {
            iced::time::every(std::time::Duration::from_millis(33)).map(|_| Message::Tick)
        } else {
            iced::Subscription::none()
        };
        iced::Subscription::batch([mouse, tick])
    }
}

fn theme(_app: &App) -> Theme {
    Theme::custom(
        "Soilad",
        Palette {
            background: Color::BLACK,
            text: Color::WHITE,
            primary: Color::from_rgb(1.0, 0.0, 0.0),
            success: Color::from_rgb(0.0, 1.0, 0.0),
            warning: Color::from_rgb(0.0, 0.0, 1.0),
            danger: Color::from_rgb(1.0, 0.0, 1.0),
        },
    )
}

fn main() -> Result<(), iced::Error> {
    iced::application(App::default, App::update, App::view)
        .theme(theme)
        .subscription(App::subscription)
        .run()
}
