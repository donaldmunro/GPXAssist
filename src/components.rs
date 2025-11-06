use eframe::egui::{self, Button, Response, Ui};
use walkers::{HttpTiles, Map, MapMemory, Plugin, Position, Projector, lon_lat, sources::OpenStreetMap};
use std::time::{Duration, Instant};

/// Walkers Plugin that renders a directional arrow showing the heading based on movement
/// from previous_position to current_position.
/// Heading is stored in degrees (0-360 range).
pub struct DirectionalArrow
//======================
{
   pub(crate) current_position:  Position,
   pub(crate) heading: f64, // Heading in degrees (0-360)
   pub(crate) wind_angle: i32, // Wind direction in degrees (0-360)
   pub(crate) wind_speed: f64 // Wind speed in metres per second

}

impl Plugin for DirectionalArrow
//===============================
{
   fn run(self: Box<Self>, ui: &mut egui::Ui, _response: &egui::Response, projector: &Projector, _map_memory: &MapMemory)
   //--------------------------------------------------------------------------------------------------------------------
   {

      // Heading is stored in degrees (0-360), convert to radians for rendering
      let bearing_rad = self.heading.to_radians();

      // Convert current position to screen coordinates
      let screen_pos = projector.project(self.current_position).to_pos2();

      // Draw the directional arrow (movement direction)
      draw_directional_arrow(ui, screen_pos, bearing_rad as f32);

      // Draw the wind arrow if wind speed is significant
      if self.wind_speed.abs() > 0.5
      {
         let wind_rad = (360.0 - self.wind_angle as f64).to_radians();
         draw_wind_arrow(ui, screen_pos, wind_rad as f32, self.wind_speed as f32);
      }
   }
}

/// Draw an arrow pointing in the specified direction (bearing in radians)
fn draw_directional_arrow(ui: &mut egui::Ui, position: egui::Pos2, bearing: f32)
//------------------------------------------------------------------------------
{
   let painter = ui.painter();

   // Arrow dimensions
   let arrow_length = 20.0;
   let arrow_width = 12.0;

   // Create arrow points (pointing upward/north initially)
   let tip = egui::Vec2::new(0.0, -arrow_length);
   let left_base = egui::Vec2::new(-arrow_width / 2.0, arrow_length / 3.0);
   let right_base = egui::Vec2::new(arrow_width / 2.0, arrow_length / 3.0);

   // Rotate points by bearing
   let rotate = |v: egui::Vec2| -> egui::Vec2 {
      let cos_b = bearing.cos();
      let sin_b = bearing.sin();
      egui::Vec2::new(v.x * cos_b - v.y * sin_b, v.x * sin_b + v.y * cos_b)
   };

   // Apply rotation and translation to world position
   let points = vec![position + rotate(tip), position + rotate(left_base), position + rotate(right_base),];

   // Draw filled arrow
   painter.add(egui::Shape::convex_polygon(points.clone(), egui::Color32::from_rgb(255, 100, 100), egui::Stroke::new(2.0, egui::Color32::WHITE)));

   // Draw a small circle at the center for visibility
   painter.circle_filled(position, 5.0, egui::Color32::from_rgb(255, 128, 128));
   painter.circle_stroke(position, 5.0, egui::Stroke::new(1.5, egui::Color32::ORANGE));
}

/// Draw a wind arrow pointing in the wind direction (bearing in radians)
/// Length is derived from wind_speed (in m/s)
/// The arrow point (tip) ends at the position (directional arrow center)
fn draw_wind_arrow(ui: &mut egui::Ui, position: egui::Pos2, wind_bearing: f32, wind_speed: f32)
//------------------------------------------------------------------------------------------------
{
   let painter = ui.painter();

   // Scale factor: 15 pixels per m/s of wind speed
   let base_length = 15.0;
   let arrow_length = base_length + (wind_speed * 15.0);
   let arrow_width = 20.0;

   // Create arrow points (pointing upward/north initially)
   let tip = egui::Vec2::new(0.0, -arrow_length);
   let left_base = egui::Vec2::new(-arrow_width / 2.0, -arrow_length * 0.6);
   let right_base = egui::Vec2::new(arrow_width / 2.0, -arrow_length * 0.6);
   let tail = egui::Vec2::new(0.0, 0.0);

   // Rotate points by wind bearing
   let rotate = |v: egui::Vec2| -> egui::Vec2 {
      let cos_b = wind_bearing.cos();
      let sin_b = wind_bearing.sin();
      egui::Vec2::new(v.x * cos_b - v.y * sin_b, v.x * sin_b + v.y * cos_b)
   };

   // Calculate offset so that the tip ends at the position
   let tip_offset = rotate(tip);
   let arrow_base_pos = position - tip_offset;

   // Apply rotation and translation so tip ends at position
   let tip_pos = position; // Tip ends at the directional arrow
   let left_pos = arrow_base_pos + rotate(left_base);
   let right_pos = arrow_base_pos + rotate(right_base);
   let tail_pos = arrow_base_pos + rotate(tail);

   // Draw the wind arrow shaft (line from tail to near the tip)
   painter.line_segment(
      [tail_pos, arrow_base_pos + rotate(egui::Vec2::new(0.0, -arrow_length * 0.65))],
      egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 150, 150))
   );

   // Draw the arrow head as a filled triangle
   let arrow_head_points = vec![tip_pos, left_pos, right_pos];
   painter.add(egui::Shape::convex_polygon(
      arrow_head_points,
      egui::Color32::from_rgb(255, 150, 150),
      egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 180, 180))
   ));

   // Add a small text label showing wind speed near the tail
   let label_pos = tail_pos - rotate(egui::Vec2::new(0.0, 12.0));
   painter.text(
      label_pos,
      egui::Align2::CENTER_CENTER,
      format!("{:.0} m/s", wind_speed),
      egui::FontId::proportional(11.0),
      egui::Color32::from_rgb(255, 180, 180)
   );
}

//-----------------------------------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToastLevel
{
   Info,
   Warning,
   Error,
   Success,
}

impl ToastLevel
{
   fn color(&self) -> egui::Color32
   {
      match self
      {
         | ToastLevel::Info => egui::Color32::from_rgb(60, 120, 216),     // Blue
         | ToastLevel::Warning => egui::Color32::from_rgb(255, 165, 0),   // Orange
         | ToastLevel::Error => egui::Color32::from_rgb(220, 53, 69),     // Red
         | ToastLevel::Success => egui::Color32::from_rgb(40, 167, 69),   // Green
      }
   }

   fn icon(&self) -> &str
   {
      match self
      {
         | ToastLevel::Info => "ℹ",
         | ToastLevel::Warning => "⚠",
         | ToastLevel::Error => "✖",
         | ToastLevel::Success => "✔",
      }
   }
}

#[derive(Clone)]
pub struct Toast
{
   message: String,
   level: ToastLevel,
   created_at: Instant,
   duration: Duration,
}

impl Toast
{
   pub fn new(message: impl Into<String>, level: ToastLevel) -> Self
   {
      Self {
         message: message.into(),
         level,
         created_at: Instant::now(),
         duration: Duration::from_secs(4),
      }
   }

   pub fn with_duration(mut self, duration: Duration) -> Self
   {
      self.duration = duration;
      self
   }

   pub fn is_expired(&self) -> bool
   {
      self.created_at.elapsed() > self.duration
   }

   pub fn remaining_time(&self) -> f32
   {
      let elapsed = self.created_at.elapsed().as_secs_f32();
      let total = self.duration.as_secs_f32();
      ((total - elapsed) / total).max(0.0)
   }
}

pub struct ToastManager
{
   toasts: Vec<Toast>,
}

impl Default for ToastManager
{
   fn default() -> Self
   {
      Self::new()
   }
}

impl ToastManager
{
   pub fn new() -> Self
   {
      Self { toasts: Vec::new() }
   }

   pub fn add(&mut self, toast: Toast)
   {
      self.toasts.push(toast);
   }

   pub fn info(&mut self, message: impl Into<String>)
   {
      self.add(Toast::new(message, ToastLevel::Info));
   }

   pub fn warning(&mut self, message: impl Into<String>)
   {
      self.add(Toast::new(message, ToastLevel::Warning));
   }

   pub fn error(&mut self, message: impl Into<String>)
   {
      self.add(Toast::new(message, ToastLevel::Error));
   }

   pub fn success(&mut self, message: impl Into<String>)
   {
      self.add(Toast::new(message, ToastLevel::Success));
   }

   pub fn show(&mut self, ctx: &egui::Context)
   {
      // Remove expired toasts
      self.toasts.retain(|toast| !toast.is_expired());

      if self.toasts.is_empty()
      {
         return;
      }

      let screen_rect = ctx.content_rect();
      let toast_width = 350.0;
      let toast_spacing = 10.0;
      let margin = 20.0;

      // Position toasts in the top-right corner
      let mut y_offset = margin;

      for (index, toast) in self.toasts.iter().enumerate()
      {
         let toast_id = egui::Id::new("toast").with(index);

         egui::Area::new(toast_id)
            .fixed_pos(egui::pos2(
               screen_rect.right() - toast_width - margin,
               screen_rect.top() + y_offset,
            ))
            .order(egui::Order::Foreground)
            .show(ctx, |ui|
            {
               egui::Frame::new()
                  .fill(egui::Color32::from_black_alpha(230))
                  .stroke(egui::Stroke::new(2.0, toast.level.color()))
                  .corner_radius(8.0)
                  .inner_margin(12.0)
                  .show(ui, |ui|
                  {
                     ui.set_width(toast_width - 24.0);

                     ui.horizontal(|ui|
                     {
                        // Icon
                        ui.label(
                           egui::RichText::new(toast.level.icon())
                              .color(toast.level.color())
                              .size(24.0),
                        );

                        ui.add_space(8.0);

                        // Message
                        ui.vertical(|ui|
                        {
                           ui.label(
                              egui::RichText::new(&toast.message)
                                 .color(egui::Color32::WHITE)
                                 .size(14.0),
                           );
                        });
                     });

                     // Progress bar showing remaining time
                     let remaining = toast.remaining_time();
                     ui.add_space(4.0);
                     let progress_bar_height = 3.0;
                     let (rect, _response) = ui.allocate_exact_size(
                        egui::vec2(toast_width - 24.0, progress_bar_height),
                        egui::Sense::hover(),
                     );

                     ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                           rect.min,
                           egui::vec2((toast_width - 24.0) * remaining, progress_bar_height),
                        ),
                        0.0,
                        toast.level.color().linear_multiply(0.8),
                     );
                  });
            });

         y_offset += 80.0 + toast_spacing;
      }

      // Request repaint to animate the progress bar
      ctx.request_repaint();
   }
}

fn toggle_button(ui: &mut Ui, text: &str, state: &mut bool) -> Response 
//---------------------------------------------------------------------
{
   let mut button = Button::new(text);
   if *state 
   {
      button = button.selected(true);
   }
   let response = ui.add(button);
   if response.clicked() 
   {
      *state = !*state;
   }
   response
}