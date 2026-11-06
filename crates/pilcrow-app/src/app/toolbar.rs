//! The toolbar below the menu, with icons drawn from lines and shapes (no image files).

use std::f32::consts::PI;

use eframe::egui::{Color32, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use super::PilcrowApp;
use super::command::Command;
use crate::settings::Flag;

const BLUE: Color32 = Color32::from_rgb(0x3B, 0x82, 0xF6);
const GREEN: Color32 = Color32::from_rgb(0x22, 0xA3, 0x5A);
const RED: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);
const AMBER: Color32 = Color32::from_rgb(0xF2, 0xA9, 0x00);
const PURPLE: Color32 = Color32::from_rgb(0x8B, 0x5C, 0xF6);

/// Icons are designed on a 16 × 16 grid.
const GRID: f32 = 16.0;
const ICON_SIZE: f32 = 21.0;
const BUTTON_SIZE: (f32, f32) = (30.0, 28.0);

#[derive(Clone, Copy)]
enum Icon {
    New,
    Open,
    Save,
    SaveAll,
    Close,
    Print,
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    Find,
    Replace,
    ZoomIn,
    ZoomOut,
    WordWrap,
    AllChars,
    IndentGuides,
    FoldAll,
    UnfoldAll,
    Split,
    DocumentMap,
    FunctionList,
    FileTree,
    Record,
    Stop,
    Play,
}

/// Draws on the icon grid inside `rect`.
struct Pen<'a> {
    painter: &'a Painter,
    origin: Pos2,
    scale: f32,
    ink: Color32,
    /// Fill behind shapes drawn on top of others.
    paper: Color32,
    /// Multiplies accent colors, e.g. to dim disabled icons.
    accent_alpha: f32,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.scale
    }

    fn stroke(&self, color: Color32) -> Stroke {
        Stroke::new(1.4 * self.scale, color)
    }

    fn accent(&self, color: Color32) -> Color32 {
        color.gamma_multiply(self.accent_alpha)
    }

    fn line(&self, points: &[(f32, f32)], color: Color32) {
        let points: Vec<Pos2> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(Shape::line(points, self.stroke(color)));
    }

    fn closed(&self, points: &[(f32, f32)], fill: Color32, outline: Color32) {
        let points: Vec<Pos2> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter
            .add(Shape::convex_polygon(points, fill, self.stroke(outline)));
    }

    fn filled(&self, points: &[(f32, f32)], fill: Color32) {
        let points: Vec<Pos2> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter
            .add(Shape::convex_polygon(points, fill, Stroke::NONE));
    }

    fn rect(&self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), fill: Color32, outline: Color32) {
        let rect = Rect::from_min_max(self.p(x0, y0), self.p(x1, y1));
        self.painter.rect_filled(rect, 1.5 * self.scale, fill);
        self.painter.rect_stroke(
            rect,
            1.5 * self.scale,
            self.stroke(outline),
            eframe::egui::StrokeKind::Middle,
        );
    }

    fn circle(&self, (x, y): (f32, f32), radius: f32, fill: Color32, outline: Stroke) {
        self.painter
            .circle(self.p(x, y), radius * self.scale, fill, outline);
    }

    /// Points on a circle from `from` to `to` degrees (counter-clockwise, 0° pointing right).
    fn arc(&self, (cx, cy): (f32, f32), radius: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
        let steps = 16;
        (0..=steps)
            .map(|i| {
                let angle = (from + (to - from) * i as f32 / steps as f32) * PI / 180.0;
                (cx + radius * angle.cos(), cy - radius * angle.sin())
            })
            .collect()
    }

    fn page(&self) {
        let ink = self.ink;
        self.closed(
            &[
                (3.0, 1.0),
                (10.0, 1.0),
                (13.0, 4.0),
                (13.0, 15.0),
                (3.0, 15.0),
            ],
            self.paper,
            ink,
        );
        self.line(&[(10.0, 1.0), (10.0, 4.0), (13.0, 4.0)], ink);
    }

    fn badge(&self, center: (f32, f32), color: Color32) {
        self.circle(
            center,
            3.6,
            self.accent(color),
            Stroke::new(1.2 * self.scale, self.paper),
        );
    }

    fn magnifier(&self, center: (f32, f32), radius: f32, handle: Color32) {
        self.circle(center, radius, Color32::TRANSPARENT, self.stroke(self.ink));
        let start = center.0 + radius * 0.75;
        let end = start + radius * 0.85;
        let handle_stroke = Stroke::new(2.4 * self.scale, self.accent(handle));
        self.painter.line_segment(
            [
                self.p(start, center.1 + radius * 0.75),
                self.p(end, center.1 + radius * 0.75 + radius * 0.85),
            ],
            handle_stroke,
        );
    }

    fn tray_with_arrow(&self, arrows: &[f32]) {
        self.line(
            &[(2.0, 10.0), (2.0, 14.0), (14.0, 14.0), (14.0, 10.0)],
            self.ink,
        );
        let blue = self.accent(BLUE);
        for &x in arrows {
            self.line(&[(x, 2.0), (x, 10.5)], blue);
            self.line(&[(x - 3.0, 7.5), (x, 10.5), (x + 3.0, 7.5)], blue);
        }
    }

    fn curved_arrow(&self, mirrored: bool) {
        let color = self.accent(PURPLE);
        let mut points = self.arc((8.0, 9.0), 5.0, -40.0, 180.0);
        let mut head = [(1.0, 6.5), (3.0, 9.6), (5.6, 7.0)];
        if mirrored {
            for (x, _) in points.iter_mut().chain(head.iter_mut()) {
                *x = GRID - *x;
            }
        }
        self.line(&points, color);
        self.line(&head, color);
    }

    fn draw(&self, icon: Icon) {
        let ink = self.ink;
        match icon {
            Icon::New => {
                self.page();
                self.badge((11.5, 11.5), GREEN);
                let white = Color32::WHITE;
                self.line(&[(11.5, 9.5), (11.5, 13.5)], white);
                self.line(&[(9.5, 11.5), (13.5, 11.5)], white);
            }
            Icon::Open => {
                self.filled(
                    &[(1.0, 5.0), (14.0, 5.0), (14.0, 13.0), (1.0, 13.0)],
                    self.paper,
                );
                let back: Vec<Pos2> = [
                    (1.0, 3.5),
                    (6.0, 3.5),
                    (7.5, 5.0),
                    (14.0, 5.0),
                    (14.0, 13.0),
                    (1.0, 13.0),
                ]
                .iter()
                .map(|&(x, y)| self.p(x, y))
                .collect();
                self.painter.add(Shape::closed_line(back, self.stroke(ink)));
                self.closed(
                    &[(3.0, 7.5), (15.5, 7.5), (13.5, 13.0), (1.0, 13.0)],
                    self.accent(AMBER),
                    ink,
                );
            }
            Icon::Save => self.tray_with_arrow(&[8.0]),
            Icon::SaveAll => self.tray_with_arrow(&[5.0, 11.0]),
            Icon::Close => {
                self.page();
                self.badge((11.5, 11.5), RED);
                let white = Color32::WHITE;
                self.line(&[(10.0, 10.0), (13.0, 13.0)], white);
                self.line(&[(13.0, 10.0), (10.0, 13.0)], white);
            }
            Icon::Print => {
                self.rect((4.5, 1.5), (11.5, 6.0), self.paper, ink);
                self.rect((1.5, 6.0), (14.5, 11.5), self.accent(BLUE), ink);
                self.rect((4.5, 9.5), (11.5, 15.0), self.paper, ink);
                self.line(&[(6.0, 12.0), (10.0, 12.0)], ink);
                self.circle((12.5, 8.0), 0.9, self.paper, Stroke::NONE);
            }
            Icon::Cut => {
                let red = self.accent(RED);
                self.line(&[(6.4, 10.6), (11.5, 1.5)], ink);
                self.line(&[(9.6, 10.6), (4.5, 1.5)], ink);
                self.circle((4.8, 12.6), 2.2, Color32::TRANSPARENT, self.stroke(red));
                self.circle((11.2, 12.6), 2.2, Color32::TRANSPARENT, self.stroke(red));
            }
            Icon::Copy => {
                self.rect((1.5, 1.5), (10.0, 10.0), self.paper, ink);
                self.rect((6.0, 6.0), (14.5, 14.5), self.paper, self.accent(BLUE));
                self.line(&[(8.5, 9.0), (12.0, 9.0)], self.accent(BLUE));
                self.line(&[(8.5, 11.5), (12.0, 11.5)], self.accent(BLUE));
            }
            Icon::Paste => {
                self.rect((2.5, 3.0), (13.5, 15.0), self.accent(AMBER), ink);
                self.rect((5.5, 1.5), (10.5, 4.5), self.paper, ink);
                self.rect((5.0, 7.0), (11.0, 13.0), self.paper, ink);
            }
            Icon::Undo => self.curved_arrow(false),
            Icon::Redo => self.curved_arrow(true),
            Icon::Find => self.magnifier((6.5, 6.5), 4.5, BLUE),
            Icon::Replace => {
                self.magnifier((5.5, 5.5), 3.8, BLUE);
                let green = self.accent(GREEN);
                self.line(&[(7.0, 13.5), (14.5, 13.5)], green);
                self.line(&[(12.0, 11.0), (14.5, 13.5), (12.0, 16.0)], green);
            }
            Icon::ZoomIn | Icon::ZoomOut => {
                self.magnifier((6.5, 6.5), 4.8, GREEN);
                self.line(&[(4.3, 6.5), (8.7, 6.5)], ink);
                if matches!(icon, Icon::ZoomIn) {
                    self.line(&[(6.5, 4.3), (6.5, 8.7)], ink);
                }
            }
            Icon::WordWrap => {
                self.line(&[(1.5, 3.0), (14.5, 3.0)], ink);
                self.line(&[(1.5, 13.0), (5.5, 13.0)], ink);
                let blue = self.accent(BLUE);
                let mut path = vec![(1.5, 7.5)];
                path.extend(self.arc((11.5, 10.25), 2.75, 90.0, -90.0));
                path.push((8.0, 13.0));
                self.line(&path, blue);
                self.line(&[(10.0, 11.0), (8.0, 13.0), (10.0, 15.0)], blue);
            }
            Icon::AllChars => {
                let blue = self.accent(BLUE);
                self.circle((6.2, 5.8), 3.3, blue, Stroke::NONE);
                self.filled(&[(6.2, 2.5), (9.0, 2.5), (9.0, 9.1), (6.2, 9.1)], blue);
                self.line(&[(6.2, 2.5), (13.0, 2.5)], ink);
                self.line(&[(9.2, 2.5), (9.2, 14.5)], ink);
                self.line(&[(12.2, 2.5), (12.2, 14.5)], ink);
            }
            Icon::IndentGuides => {
                let purple = self.accent(PURPLE);
                for x in [2.5, 6.5] {
                    for y in [1.5, 5.5, 9.5, 13.5] {
                        self.line(&[(x, y), (x, y + 2.0)], purple);
                    }
                }
                self.line(&[(4.5, 3.0), (14.5, 3.0)], ink);
                self.line(&[(8.5, 8.0), (14.5, 8.0)], ink);
                self.line(&[(8.5, 13.0), (13.0, 13.0)], ink);
            }
            Icon::FoldAll | Icon::UnfoldAll => {
                let green = self.accent(GREEN);
                for top in [1.5, 9.0] {
                    self.rect((1.5, top), (6.5, top + 5.0), self.paper, green);
                    self.line(&[(2.8, top + 2.5), (5.2, top + 2.5)], green);
                    if matches!(icon, Icon::UnfoldAll) {
                        self.line(&[(4.0, top + 1.3), (4.0, top + 3.7)], green);
                    }
                    self.line(&[(8.5, top + 2.5), (14.5, top + 2.5)], ink);
                }
            }
            Icon::Split => {
                self.rect((1.5, 2.5), (14.5, 13.5), self.paper, ink);
                self.filled(
                    &[(8.6, 3.2), (13.8, 3.2), (13.8, 12.8), (8.6, 12.8)],
                    self.accent(BLUE).gamma_multiply(0.5),
                );
                self.line(&[(8.0, 2.5), (8.0, 13.5)], ink);
            }
            Icon::DocumentMap => {
                self.rect((3.0, 1.0), (13.0, 15.0), self.paper, ink);
                self.filled(
                    &[(3.8, 6.0), (12.2, 6.0), (12.2, 10.0), (3.8, 10.0)],
                    self.accent(BLUE).gamma_multiply(0.6),
                );
                for (y, end) in [(3.5, 10.5), (8.0, 9.0), (12.5, 11.0)] {
                    self.line(&[(5.0, y), (end, y)], ink);
                }
            }
            Icon::FunctionList => {
                let purple = self.accent(PURPLE);
                for y in [3.0, 8.0, 13.0] {
                    self.circle((3.0, y), 1.5, purple, Stroke::NONE);
                    self.line(&[(6.0, y), (14.5, y)], ink);
                }
            }
            Icon::FileTree => {
                let amber = self.accent(AMBER);
                self.line(&[(3.5, 4.0), (3.5, 13.0), (7.0, 13.0)], ink);
                self.line(&[(3.5, 8.5), (7.0, 8.5)], ink);
                self.rect((1.5, 1.0), (6.0, 4.0), amber, ink);
                self.rect((7.5, 7.0), (14.5, 10.0), amber, ink);
                self.rect((7.5, 11.5), (14.5, 14.5), self.paper, ink);
            }
            Icon::Record => self.circle((8.0, 8.0), 5.0, self.accent(RED), Stroke::NONE),
            Icon::Stop => self.rect((3.5, 3.5), (12.5, 12.5), self.accent(RED), self.accent(RED)),
            Icon::Play => {
                self.closed(
                    &[(4.0, 2.5), (13.5, 8.0), (4.0, 13.5)],
                    self.accent(GREEN),
                    self.accent(GREEN),
                );
            }
        }
    }
}

impl PilcrowApp {
    /// The toolbar; returns the command of a clicked icon.
    pub(super) fn toolbar(&self, ui: &mut Ui) -> Option<Command> {
        let record = if self.recording.is_some() {
            (Icon::Stop, Command::StopRecording)
        } else {
            (Icon::Record, Command::StartRecording)
        };
        let groups: [&[(Icon, Command)]; 9] = [
            &[
                (Icon::New, Command::New),
                (Icon::Open, Command::Open),
                (Icon::Save, Command::Save),
                (Icon::SaveAll, Command::SaveAll),
                (Icon::Close, Command::Close),
                (Icon::Print, Command::Print),
            ],
            &[
                (Icon::Cut, Command::Cut),
                (Icon::Copy, Command::Copy),
                (Icon::Paste, Command::Paste),
            ],
            &[(Icon::Undo, Command::Undo), (Icon::Redo, Command::Redo)],
            &[
                (Icon::Find, Command::OpenFind),
                (Icon::Replace, Command::OpenReplace),
            ],
            &[
                (Icon::ZoomIn, Command::ZoomIn),
                (Icon::ZoomOut, Command::ZoomOut),
            ],
            &[
                (Icon::WordWrap, Command::Toggle(Flag::WordWrap)),
                (Icon::AllChars, Command::ShowAllCharacters),
                (Icon::IndentGuides, Command::Toggle(Flag::IndentGuides)),
            ],
            &[
                (Icon::FoldAll, Command::FoldAll),
                (Icon::UnfoldAll, Command::UnfoldAll),
            ],
            &[
                (Icon::Split, Command::ToggleSplit),
                (Icon::DocumentMap, Command::Toggle(Flag::DocumentMap)),
                (Icon::FunctionList, Command::Toggle(Flag::FunctionList)),
                (Icon::FileTree, Command::Toggle(Flag::FileTree)),
            ],
            &[record, (Icon::Play, Command::PlayMacro)],
        ];
        let texts = self.texts();
        let mut picked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (index, group) in groups.into_iter().enumerate() {
                if index > 0 {
                    ui.add_space(3.0);
                    ui.separator();
                    ui.add_space(3.0);
                }
                for &(icon, command) in group {
                    let mut tooltip = command.label(texts).trim_end_matches('…').to_owned();
                    if let Some(shortcut) = self.keymap.get(command) {
                        tooltip = format!("{tooltip} ({})", ui.ctx().format_shortcut(&shortcut));
                    }
                    let enabled = self.enabled(command);
                    if icon_button(ui, icon, &tooltip, enabled, self.is_on(command)) {
                        picked = Some(command);
                    }
                }
            }
        });
        picked
    }
}

fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str, enabled: bool, on: bool) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON_SIZE.0, BUTTON_SIZE.1), sense);
    let visuals = ui.visuals();
    if on {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.selection.bg_fill.gamma_multiply(0.45));
    }
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let icon_rect = Rect::from_center_size(rect.center(), vec2(ICON_SIZE, ICON_SIZE));
    let ink = if enabled {
        visuals.text_color()
    } else {
        visuals.weak_text_color().gamma_multiply(0.5)
    };
    let pen = Pen {
        painter: ui.painter(),
        origin: pos2(icon_rect.left(), icon_rect.top()),
        scale: ICON_SIZE / GRID,
        ink,
        paper: visuals.extreme_bg_color,
        accent_alpha: if enabled { 1.0 } else { 0.35 },
    };
    pen.draw(icon);
    let clicked = response.clicked();
    response.on_hover_text(tooltip);
    clicked
}
