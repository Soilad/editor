use iced::widget::pane_grid;

fn test() {
    let _ = pane_grid::ResizeEvent { pane: pane_grid::Pane(0), split: pane_grid::Split::Horizontal, ratio: 0.5 };
}
