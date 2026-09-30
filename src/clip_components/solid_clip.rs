use std::time::Duration;

use avio::Command;
use iced::Element;
use iced::widget::{button, container, row, text};
use iced_aw::ColorPicker;

use crate::clip_components::{ClipComponent, ClipProperties, open_preview};
use crate::preview::Preview;
use crate::widgets::theme::button_style;
use crate::helper_funcs::to_iced_color;
use crate::{Message, PropertiesDialog};

/// A generated full-frame colour layer. Like text, a solid has no intrinsic
/// length, so it is always trimmed to an explicit `duration`.
#[derive(Debug, Clone)]
pub struct SolidComponent {
    pub clip: avio::Clip,
}

impl SolidComponent {
    pub fn new(color: avio::Color, duration: Duration) -> Self {
        Self {
            clip: avio::Clip::solid(color).trim(Duration::ZERO, duration),
        }
    }

    /// The colour this clip fills the frame with.
    pub fn color(&self) -> Option<avio::Color> {
        match &self.clip.source {
            avio::ClipSource::Solid(color) => Some(*color),
            _ => None,
        }
    }
}

impl ClipComponent for SolidComponent {
    fn preview(&self) -> Result<Preview, String> {
        open_preview(vec![self.clip.clone().offset(Duration::ZERO)], Vec::new())
    }

    /// The fill colour.
    fn properties(&self) -> ClipProperties {
        ClipProperties {
            color: self.color(),
            ..ClipProperties::default()
        }
    }

    fn set_properties(&self, properties: &ClipProperties) -> Option<Command> {
        let color = properties.color?;
        let mut clip = self.clip.clone();
        clip.source = avio::ClipSource::Solid(color);
        Some(Command::SetClip {
            clip: clip.id,
            value: Box::new(clip),
        })
    }

    fn avio_clip(&self) -> &avio::Clip {
        &self.clip
    }

    fn properties_view<'a>(&self, dialog: &'a PropertiesDialog) -> Option<Element<'a, Message>> {
        if let Some(color) = dialog.values.color {
            let color = to_iced_color(color);
            let swatch = container(text(""))
                .width(24)
                .height(24)
                .style(move |_| container::background(color));
            let picker = ColorPicker::new(
                dialog.picking_color,
                color,
                button("Colour...")
                    .style(button_style)
                    .on_press(Message::PickColor),
                Message::CancelColor,
                Message::ColorPicked,
            );
            Some(row![swatch, picker].spacing(10).into())
        } else {
            None
        }
    }
}
