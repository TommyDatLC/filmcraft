//! Plain local-build identification. Modified builds do not embed the ArtCraft trademark artwork
//! (AGENTS.md §1.8). The upstream app icon remains openly licensed.

/// Draw a plain text label and return its rect, retaining the existing layout contract.
pub fn paint_wordmark(ui: &egui::Ui, left_center: egui::Pos2, height: f32, _dark: bool) -> Option<egui::Rect> {
    let text = ui.painter().layout_no_wrap("FilmCraft Custom".into(), egui::FontId::proportional(height), ui.visuals().text_color());
    let rect = egui::Rect::from_min_size(egui::pos2(left_center.x, left_center.y - text.size().y / 2.0), text.size());
    ui.painter().galley(rect.min, text, ui.visuals().text_color());
    Some(rect)
}
