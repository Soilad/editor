use std::time::Duration;

use avio::Command;
use iced::widget::{text, text_input};

use crate::clip_components::{ClipComponent, ClipProperties, open_preview};
use crate::preview::Preview;
use crate::{Message, PropertiesDialog};

/// A generated title layer. Text has no intrinsic length, so it is always
/// trimmed to an explicit `duration`.
#[derive(Debug, Clone)]
pub struct TextComponent {
    pub clip: avio::Clip,
}

impl TextComponent {
    pub fn new(spec: avio::TextSpec, duration: Duration) -> Self {
        Self {
            clip: avio::Clip::text(spec).trim(Duration::ZERO, duration),
        }
    }

    /// The text spec this clip renders.
    pub fn spec(&self) -> Option<&avio::TextSpec> {
        match &self.clip.source {
            avio::ClipSource::Text(spec) => Some(spec),
            _ => None,
        }
    }
}

impl ClipComponent for TextComponent {
    fn preview(&self) -> Result<Preview, String> {
        open_preview(vec![self.clip.clone().offset(Duration::ZERO)], Vec::new())
    }

    /// The text and its colour.
    fn properties(&self) -> ClipProperties {
        let spec = self.spec();
        ClipProperties {
            text: spec.map(|spec| spec.text.clone()),
            color: spec.map(|spec| spec.style.color),
        }
    }

    fn set_properties(&self, properties: &ClipProperties) -> Option<Command> {
        let mut spec = self.spec()?.clone();
        if let Some(text) = &properties.text {
            spec.text = text.clone();
        }
        if let Some(color) = properties.color {
            spec.style.color = color;
        }
        let mut clip = self.clip.clone();
        clip.source = avio::ClipSource::Text(spec);
        Some(Command::SetClip {
            clip: clip.id,
            value: Box::new(clip),
        })
    }

    fn avio_clip(&self) -> &avio::Clip {
        &self.clip
    }

    fn properties_view<'a>(
        &self,
        dialog: &'a PropertiesDialog,
    ) -> Option<iced::Element<'a, Message>> {
        if let Some(value) = &dialog.values.text {
            let a = text_input("Text", value)
                .on_input(Message::PropertiesTextChanged)
                .on_submit(Message::ApplyProperties);
            Some(a.into())
        } else {
            None
        }
    }
}
