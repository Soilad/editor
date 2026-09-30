use std::sync::LazyLock;

use iced::{
    Background, Border, Color,
    border::Radius,
    theme::{Palette, Theme},
    widget::button::{Status, Style},
};

const _RADIUS: f32 = 20.0;
pub const CORNER_RADIUS: Radius = Radius {
    top_left: _RADIUS,
    top_right: _RADIUS,
    bottom_right: _RADIUS,
    bottom_left: _RADIUS,
};

// `Theme::custom` allocates, so it can't run in a const context.
pub static THEME: LazyLock<Theme> = LazyLock::new(|| {
    Theme::GruvboxDark
    // Theme::custom(
    //     "Soilad",
    //     Palette {
    //         background: Color::BLACK,
    //         text: Color::WHITE,
    //         primary: Color::from_rgb(1.0, 0.0, 0.0),
    //         success: Color::from_rgb(0.0, 1.0, 0.0),
    //         warning: Color::from_rgb(0.0, 0.0, 1.0),
    //         danger: Color::from_rgb(1.0, 0.0, 1.0),
    //     },
    // )
});

// `.style()` takes a `Fn(&Theme, Status) -> Style`, not a plain `Style`.
pub fn button_style(theme: &Theme, _status: Status) -> Style {
    let palette = theme.palette();
    Style {
        text_color: palette.text,
        border: Border {
            color: palette.primary,
            width: 1.0,
            radius: CORNER_RADIUS,
        },
        background: Some(Background::Color(palette.background)),
        ..Style::default()
    }
}
