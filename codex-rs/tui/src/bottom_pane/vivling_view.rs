use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::render::renderable::Renderable;
use crate::vivling::VivlingPanelData;

use super::CancellationEvent;
use super::bottom_pane_view::BottomPaneView;
use super::bottom_pane_view::ViewCompletion;
use super::popup_consts::standard_popup_hint_line;

pub(crate) struct VivlingCardView {
    data: VivlingPanelData,
    completion: Option<ViewCompletion>,
}

pub(crate) struct VivlingUpgradeView {
    data: VivlingPanelData,
    completion: Option<ViewCompletion>,
}

impl VivlingCardView {
    pub(crate) fn new(data: VivlingPanelData) -> Self {
        Self {
            data,
            completion: None,
        }
    }
}

impl VivlingUpgradeView {
    pub(crate) fn new(data: VivlingPanelData) -> Self {
        Self {
            data,
            completion: None,
        }
    }
}

macro_rules! impl_vivling_modal {
    ($ty:ident, $view_id:literal) => {
        impl BottomPaneView for $ty {
            fn handle_key_event(&mut self, key_event: KeyEvent) {
                if matches!(key_event.code, KeyCode::Esc | KeyCode::Enter) {
                    self.completion = Some(ViewCompletion::Accepted);
                }
            }

            fn on_ctrl_c(&mut self) -> CancellationEvent {
                self.completion = Some(ViewCompletion::Cancelled);
                CancellationEvent::Handled
            }

            fn is_complete(&self) -> bool {
                self.completion.is_some()
            }

            fn completion(&self) -> Option<ViewCompletion> {
                self.completion
            }

            fn view_id(&self) -> Option<&'static str> {
                Some($view_id)
            }
        }

        impl Renderable for $ty {
            fn desired_height(&self, width: u16) -> u16 {
                let lines = if width < 88 {
                    &self.data.narrow_lines
                } else {
                    &self.data.wide_lines
                };
                lines.len() as u16 + 4
            }

            fn render(&self, area: Rect, buf: &mut Buffer) {
                if area.height == 0 || area.width == 0 {
                    return;
                }
                Clear.render(area, buf);
                let lines = if area.width < 88 {
                    &self.data.narrow_lines
                } else {
                    &self.data.wide_lines
                };

                Paragraph::new(Line::from(self.data.title.clone().bold())).render(
                    Rect {
                        x: area.x,
                        y: area.y,
                        width: area.width,
                        height: 1,
                    },
                    buf,
                );

                for (index, line) in lines.iter().enumerate() {
                    let y = area.y.saturating_add(1 + index as u16);
                    if y >= area.y.saturating_add(area.height).saturating_sub(1) {
                        break;
                    }
                    Paragraph::new(Line::from(line.clone())).render(
                        Rect {
                            x: area.x,
                            y,
                            width: area.width,
                            height: 1,
                        },
                        buf,
                    );
                }

                let hint_y = area.y.saturating_add(area.height.saturating_sub(1));
                Paragraph::new(standard_popup_hint_line()).render(
                    Rect {
                        x: area.x,
                        y: hint_y,
                        width: area.width,
                        height: 1,
                    },
                    buf,
                );
            }
        }
    };
}

impl_vivling_modal!(VivlingCardView, "vivling-card");
impl_vivling_modal!(VivlingUpgradeView, "vivling-upgrade");
