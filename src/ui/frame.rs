use std::{
   future::Future, path::PathBuf, sync::{Arc, atomic::Ordering, mpsc::Sender}, time::Duration
};

use eframe::egui::{self, Color32, ColorImage, Context, Frame, Image, Vec2, TextureHandle};

use walkers::{lon_lat, Map};

use crate::{
   components::DirectionalArrow,
   data::{ INVALID_COORDINATE, RiderDataJSON},
   gpx::{TrackPoint, WGS84Position, calculate_bearing, find_closest_point, process_gpx, process_fit},
};
use eframe::emath::Numeric;
use crate::SETTINGS;
use crate::settings::Settings;

use super::ui::{GPXAssistUI, ViewMode};

const MIN_GRADIENT_DELTA: i64 = 1;
const MAX_GRADIENT_DELTA: i64 = 100;
const MIN_GRADIENT_LENGTH: i64 = 100;
const MAX_GRADIENT_LENGTH: i64 = 10000;
const MIN_GRADIENT_POSITION: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq)]
enum PositionOrigin
{
   GPX,
   TPV,
   NONE
}

impl eframe::App for GPXAssistUI
//==============================
{
   #[rustfmt::skip]
   fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame)
   //------------------------------------------------------------------
   {
      set_style(ctx);
      egui::TopBottomPanel::top("top_panel").resizable(true).min_height(36.0)
      .frame(Frame::new().fill(egui::Color32::from_rgb(169, 157, 133)))
      .show(ctx, |ui|
      {
         if let Ok(tt) = self.open_dialog_channel.1.try_recv() // new GPX file opened
         {
            let (trackdata, filepath, err) = tt;
            if ! trackdata.is_empty()
            {
               self.gpx_file = Some(PathBuf::from(&filepath));
               // self.current_position = trackdata.first().copied();
               self.gpx_track = Arc::new(trackdata);
               self.track_total_distance = self.gpx_track.last().map_or(0.0, |p| p.distance);
               self.current_mode.store(ViewMode::Gradient);
               self.is_simulating.store(false, Ordering::Relaxed);
               self.is_first_map_frame = false;
               self.is_first_street_frame = false;
               self.is_first_gradient_frame = true;
               match PathBuf::from(&filepath).file_name()
               {
                  | Some(name) =>
                  {
                     let title = "GPXAssist: ".to_string() + &name.to_string_lossy();
                     ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                  },
                  | None => ()
               }               
               self.start_distance_thread(ctx);
            }
            else if ! err.trim().is_empty()
            {
               self.toast_manager.error(&err, None);
            }
            else
            {
               self.toast_manager.error("The selected GPX file contains no track points or could not be processed.", None);
            }
         }

         //Draw the top toolbar
         ui.horizontal(|ui|
         {
            if let Some((texture, size)) = self.textures.get("settings")
               && ui.add(egui::Button::image(egui::Image::new(texture)
                     .alt_text("Settings")
                     .bg_fill(egui::Color32::from_rgb(232, 227, 209))
                     .fit_to_exact_size((*size).into()))).clicked()
            {
               let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
               let mut settings_lock = settings.lock();
               settings_lock.open_settings_dialog(self);
            }

            ui.separator();

            let mut dist: i64 = self.requested_delta.load();
            ui.label(egui::RichText::new("Refresh:").color(egui::Color32::YELLOW).strong());
            let distance_response = ui.add_sized(
               egui::Vec2::new(80.0, 30.0), // Fixed size: width = 80, height = 30
               egui::DragValue::new(&mut dist)
                  .suffix("m")
                  .range(0.0..=1000.0)
                  .min_decimals(0)
                  .max_decimals(0)
                  .speed(1.0)
                  .clamp_existing_to_range(true))
            .on_hover_text("The distance in metres to travel before updating the current view.")
            .on_hover_text("Drag with mouse or enter a value.");
            if distance_response.dragged() || distance_response.changed()
            {
               self.requested_delta.store(dist);
               // println!("Requested Distance Delta set to {:.2} meters", dist);
            }
            ui.separator();

            let mut current_mode = self.current_mode.load();
            let before_mode = self.current_mode.load();
            ui.selectable_value(&mut current_mode, ViewMode::Map,
               egui::RichText::new("Map").color(egui::Color32::LIGHT_YELLOW));
            ui.selectable_value(&mut current_mode, ViewMode::StreetView,
               egui::RichText::new("StreetView").color(egui::Color32::LIGHT_YELLOW));
            ui.selectable_value(&mut current_mode, ViewMode::Gradient,
               egui::RichText::new("Gradient").color(egui::Color32::LIGHT_YELLOW));
            if before_mode != current_mode
            {
               self.current_mode.store(current_mode);
               if before_mode == ViewMode::Map
               {
                  self.is_first_map_frame = false;
               }
               if before_mode == ViewMode::StreetView
               {
                  self.is_first_street_frame = false;
               }
               if before_mode == ViewMode::Gradient
               {
                  self.is_first_gradient_frame = false;
               }
               if current_mode == ViewMode::Map
               {
                  self.is_first_map_frame = true;
               }
               if current_mode == ViewMode::StreetView
               {
                  self.is_first_street_frame = true;
               }
               if current_mode == ViewMode::Gradient
               {
                  self.is_first_gradient_frame = true;
               }
            }
            ui.separator();
            ui.add_space(100.0);

            if let Some((texture, size)) = self.textures.get("open")
               && ui.add(egui::Button::image(egui::Image::new(texture)
                     .alt_text("Open")
                     .bg_fill(egui::Color32::from_rgb(232, 227, 209))
                     .fit_to_exact_size((*size).into()))).clicked()
            {
               let sender = self.open_dialog_channel.0.clone();
               open_file_dialog(ui.ctx(), sender);
            }

            let mut speed: f64 = self.simulated_speed.load();
            ui.label(egui::RichText::new("Speed:").color(egui::Color32::YELLOW).strong());
            let speed_response = ui.add_sized(
               egui::Vec2::new(60.0, 30.0), // Fixed size: width = 60, height = 30
               egui::DragValue::new(&mut speed)
                  .range(5.0..=300.0)
                  .min_decimals(0)
                  .max_decimals(0)
                  .speed(1.0)
                  .clamp_existing_to_range(true))
            .on_hover_text("The speed in km/h when simulating. Drag with mouse or enter a value.");
            if speed_response.dragged() || speed_response.changed()
            {
               speed = speed.clamp(5.0, 300.0);
               self.simulated_speed.store(speed);
               println!("Simulated speed set to {:.2} meters", speed);
            }

            if self.is_simulating.load(Ordering::Relaxed)
            {
               if let Some((texture, size)) = self.textures.get("test-off")
                  && ui.add(egui::Button::image(egui::Image::new(texture)
                     .alt_text("Stop Test")
                     .bg_fill(egui::Color32::from_rgb(190, 190, 190))
                     .fit_to_exact_size((*size).into())).selected(true))
                     .on_hover_text("Stop simulating movement along the GPX track.")
               .clicked()
               {  // Stop Simulation button
                  self.is_simulating.store(false, Ordering::Relaxed);
               }
            }
            else if  ! self.is_simulating.load(Ordering::Relaxed)
                     && let Some((texture, size)) = self.textures.get("test-on")
                     && ui.add(egui::Button::image(egui::Image::new(texture)
                           .alt_text("Test")
                           .bg_fill(egui::Color32::from_rgb(232, 227, 209))
                           .fit_to_exact_size((*size).into())).selected(false))
                           .on_hover_text("Start simulating movement along the GPX track at 45km/h.")
            .clicked()
            {
               self.start_simulation_thread(ctx);
            }
         })
      } );

      egui::CentralPanel::default()
      .show(ctx, |ui|
      {
         let (exists_broadcast_file, aged_broadcast_file) = self.check_broadcast_file();
         let broadcast_file = get_broadcast_file();
         let is_no_broadcast_file = broadcast_file.is_none() || !broadcast_file.as_ref().unwrap().is_file() ||
                                    ! exists_broadcast_file || aged_broadcast_file;
         let current_mode = self.current_mode.load();
         if current_mode == ViewMode::NA //&& is_no_broadcast_file
         {
            let available_size = ui.available_size();
            let image = Image::new(egui::include_image!("../../assets/GPXAssist.png"))
               .maintain_aspect_ratio(false)
               .fit_to_exact_size(available_size)
               .shrink_to_fit();

            ui.centered_and_justified(|ui|
            {
               ui.add(image);
            });
         }
         else if is_no_broadcast_file && ! self.is_simulating.load(Ordering::Relaxed)
         {
            let delta = self.requested_delta.load();
            display_invalid_broadcast_directory(ui, aged_broadcast_file, delta);
         }
         else
         {
            let rider = self.rider_data.load();
            let requested_delta = self.requested_delta.load();
            let is_update = rider.distance_moved() >= requested_delta;
            let gradient_delta = self.gradient_delta.load();
            let position: WGS84Position;
            let previous_position: WGS84Position;
            let position_origin: PositionOrigin;
            if ! rider.has_position
            {
               if self.gpx_file.is_some() &&
                  let (Some(gpx_position), _) = find_closest_point(&self.gpx_track, rider.distance as f64)
               {
                  previous_position = match self.previous_gpx_position
                  {
                     | Some(pos) => WGS84Position { latitude: pos.point.latitude, longitude: pos.point.longitude, altitude: pos.point.altitude },
                     | None => WGS84Position
                     {
                        latitude: INVALID_COORDINATE as f64,
                        longitude: INVALID_COORDINATE as f64,
                        altitude: INVALID_COORDINATE as f64
                     }
                  };
                  position = gpx_position.point;
                  position_origin = PositionOrigin::GPX;
               }
               else
               {
                  position = WGS84Position { latitude: INVALID_COORDINATE as f64, longitude: INVALID_COORDINATE as f64, altitude: INVALID_COORDINATE as f64 };
                  previous_position = WGS84Position { latitude: INVALID_COORDINATE as f64, longitude: INVALID_COORDINATE as f64, altitude: INVALID_COORDINATE as f64 };
                  position_origin = PositionOrigin::NONE;
               }
            }
            else
            {
               previous_position = rider.previous_position();
               position = rider.position();
               position_origin = PositionOrigin::TPV;
            }

            if current_mode == ViewMode::Map //&& is_update
                  && let (Some(tiles), Some(memory)) = (&mut self.tiles, &mut self.map_memory)
                  && position_origin != PositionOrigin::NONE
            {
               let point = lon_lat(position.longitude, position.latitude);
               let heading = calculate_bearing(previous_position.longitude, previous_position.latitude,
                                                position.longitude, position.latitude);
               ui.add(
                  Map::new(Some(tiles), memory, point)
                     .with_plugin(DirectionalArrow
                     {
                        current_position: point,
                        heading: heading,
                        wind_angle: rider.wind_angle,
                        wind_speed: rider.wind_speed.to_f64() / 1000.0 // wind speed is in mm/s so convert to m/s
                     })
               );
               if position_origin == PositionOrigin::GPX
               {
                  self.previous_gpx_position = Some(TrackPoint::from(position));
               }
            }
            else if current_mode == ViewMode::StreetView && position_origin != PositionOrigin::NONE
            {
               if self.encrypted_api_key.is_none()
               {
                  display_streetview_info(ui);
               }
               else if is_update || self.is_first_street_frame
               {
                  self.display_streetview(ctx, ui, &previous_position, &position, 120);
               }
               else if let Some(texture) = &self.streetview_texture
               {
                  ui.centered_and_justified(|ui|
                  {
                     let available_size = ui.available_size();
                     ui.add(Image::new(texture)
                              .maintain_aspect_ratio(false)
                              .fit_to_exact_size(available_size)
                              .shrink_to_fit()
                           );
                  });
               }
               if position_origin == PositionOrigin::GPX
               {
                  self.previous_gpx_position = Some(TrackPoint::from(position));
               }
            } // self.current_mode == ViewMode::StreetView
            else if  current_mode == ViewMode::Gradient
            {
               if self.gpx_file.is_none() || self.gpx_track.is_empty()
               {
                  display_open_gpx(ui, self.textures.get("open"));
               }
               else
               {
                  let is_gradient_update = (rider.distance - rider.previous_gradient_distance) >= gradient_delta;
                  if is_gradient_update || self.is_first_gradient_frame
                  {
                     // println!("Gradient Regen {:?} {}", position, updated_distance);
                     let available_size = ui.available_size();
                     let mut errmsg = String::new();
                     let total_distance = self.track_total_distance;
                     let gradient_image = match self.new_gradient_image(rider.distance, total_distance, available_size.x, available_size.y, 100)
                     {
                        | Ok(img) => Some(img),
                        | Err(msg) =>
                        {
                           eprintln!("Error calculating gradient image: {msg}");
                           self.gradient_pixmap = None;
                           errmsg = msg;
                           None
                        }
                     };
                     if let Some(color_image) = gradient_image
                     {
                        let texture_name = "gradient_image";
                        if self.gradient_texture.is_some()
                        {
                           self.gradient_texture.as_mut().unwrap().set(color_image, egui::TextureOptions::LINEAR)
                        }
                        else
                        {
                           self.gradient_texture = Some(ctx.load_texture(texture_name, color_image, Default::default() ));
                        }
                     }
                     else
                     {
                        ui.add(egui::Label::new(egui::RichText::new(errmsg).strong().color(egui::Color32::RED) ));
                     }
                     if self.gradient_texture.is_some()
                     {
                        render_current_gradient(self, ui);
                     }
                     if position_origin == PositionOrigin::GPX
                     {
                        self.previous_gpx_position = Some(TrackPoint::from(position));
                     }
                     self.gradient_distance = rider.distance;
                     self.is_first_gradient_frame = false;
                  }
                  else if is_gradient_update &&
                     let (Some(position), _) = find_closest_point(&self.gpx_track, rider.distance as f64)
                  {
                     println!("Gradient position Update {:?}", position);
                     if position.distance > 0.0
                     {
                        let available_size = ui.available_size();
                        let gradient_image = match self.draw_gradient_marker(available_size.x, available_size.y, rider.distance)
                        {
                           | Ok(img) => Some(img),
                           | Err(msg) =>
                           {
                              eprintln!("Error recalculating gradient image: {msg}");
                              None
                           }
                        };
                        if let Some(color_image) = gradient_image
                        {
                           let texture_name = "gradient_image";
                           if self.gradient_texture.is_some()
                           {
                              self.gradient_texture.as_mut().unwrap().set(color_image, egui::TextureOptions::LINEAR)
                           }
                           else
                           {
                              self.gradient_texture = Some(ctx.load_texture(texture_name, color_image, Default::default() ));
                           }
                           if position_origin == PositionOrigin::GPX
                           {
                              self.previous_gpx_position = Some(TrackPoint::from(position));
                           }
                           // self.current_distance = updated_distance;
                           self.gradient_distance = rider.distance;
                        }
                        render_current_gradient(self, ui);
                     }
                     // self.render_gradient(ui, &texture);
                  }
                  else if self.gpx_file.is_some() //&& let Some(texture) = &self.gradient_texture
                  {
                     // println!("Gradient redraw");
                     render_current_gradient(self, ui);
                  }
               }
         }
         }
      });

      if self.show_settings_dialog
      {
         let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
         let mut settings_lock = settings.lock();
         // let toast_manager = &mut self.toast_manager;
         settings_lock.show_settings_dialog(self, ctx);
      }
      else
      {
         let msg = self.settings_dialog_message.clone();
         if ! msg.is_empty()
         {
            if ! self.show_settings_dialog_err
            {
               self.toast_manager.info(&msg, Some(Duration::from_secs(3)));
            }
            else
            {
               self.toast_manager.error(&msg, None);
            }
         }
         self.settings_dialog_message.clear();
         self.show_settings_dialog_err = false;
      }

      self.toast_manager.show(ctx);
   }
}

fn display_streetview_info(ui: &mut egui::Ui)
//---------------------------------------------
{
   ui.add(egui::Label::new(
      egui::RichText::new("No Street View API key set in settings.")
         .strong()
         .color(egui::Color32::RED),
   ));
   ui.hyperlink_to(
      "Click to get a Google Maps API Key (https://console.cloud.google.com/google/maps-apis/)",
      "https://console.cloud.google.com/google/maps-apis/",
   );
   ui.separator();
   ui.add(egui::Label::new(egui::RichText::new("First 10000 StreetViews per month are free.").color(egui::Color32::GREEN)));
   ui.hyperlink_to(
      "Click for Pricing Details (https://developers.google.com/maps/billing-and-pricing/pricing#map-loads-pricing)",
      "https://developers.google.com/maps/billing-and-pricing/pricing#map-loads-pricing",
   );
   let settings_dir = Settings::get_settings_path().unwrap_or(PathBuf::from("."));
   ui.label(format!(
      "After obtaining a key, click the settings button to add API key to settings or modify the settings file {:#?} directly.",
      settings_dir
   ));
}

fn gradient_options(me: &mut GPXAssistUI, ui: &mut egui::Ui)
//----------------------------------------------------------
{
   let mut gradient_delta = me.gradient_delta.load();
   let mut gradient_length = me.gradient_length.load();
   let mut gradient_position = me.gradient_offset.load();
   let mut vertical_scale: f64 = me.vertical_scale.load();
   let mut flat_gradient: f64 = me.gradient_flat.load();
   let mut medium_gradient: f64 = me.gradient_medium.load();
   let mut extreme_gradient: f64 = me.gradient_extreme.load();
   ui.horizontal(|ui| {
      ui.label(
         egui::RichText::new("Gradient Refresh:")
            .color(egui::Color32::YELLOW)
            .strong(),
      );
      let delta_response = ui
         .add_sized(
            egui::Vec2::new(100.0, 30.0),
            egui::DragValue::new(&mut gradient_delta)
               .suffix("m")
               .range(MIN_GRADIENT_DELTA..=MAX_GRADIENT_DELTA)
               .speed(2.0),
         )
         .on_hover_text(format!(
            "The distance in metres to travel before redrawing the gradient display with rider positioned at {} (metres)",
            gradient_position
         ));
      if delta_response.dragged() || delta_response.changed()
      {
         me.gradient_delta.store(gradient_delta);
         {
            gradient_delta = gradient_delta.clamp(MIN_GRADIENT_DELTA, MAX_GRADIENT_DELTA);
            let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
            let mut settings_lock = settings.lock();
            settings_lock.gradient_delta = gradient_delta as f64;
            match settings_lock.write_settings() {
               | Ok(_) => (),
               | Err(e) => eprintln!("Error saving gradient delta to settings: {}", e),
            }
         }
         // me.is_first_gradient_frame = true;
      }

      ui.add_space(5.0);
      ui.label("Length:");
      let length_response = ui
         .add_sized(
            egui::Vec2::new(100.0, 30.0),
            egui::DragValue::new(&mut gradient_length)
               .range(MIN_GRADIENT_LENGTH..=MAX_GRADIENT_LENGTH)
               .suffix("m")
               .speed(5.0),
         )
         .on_hover_text("The length of the gradient section to display (metres)");
      if length_response.dragged() || length_response.changed()
      {
         gradient_length = gradient_length.clamp(MIN_GRADIENT_LENGTH, MAX_GRADIENT_LENGTH);
         me.gradient_length.store(gradient_length);
         me.is_first_gradient_frame = true;
         {
            let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
            let mut settings_lock = settings.lock();
            settings_lock.gradient_length = gradient_length as f64;
            match settings_lock.write_settings() {
               | Ok(_) => (),
               | Err(e) => eprintln!("Error saving gradient length to settings: {}", e),
            }
         }
      }

      ui.add_space(5.0);
      ui.label("Offset:");
      let max_gradient_position = gradient_length.clamp(50, gradient_length.clamp(MIN_GRADIENT_POSITION, MAX_GRADIENT_LENGTH - 100));
      let position_response = ui
         .add_sized(
            egui::Vec2::new(100.0, 30.0),
            egui::DragValue::new(&mut gradient_position)
               .suffix("m")
               .range(MIN_GRADIENT_POSITION..=max_gradient_position)
               .speed(5.0),
         )
         .on_hover_text("The position within the gradient section where the rider currently is positioned (metres)");
      if position_response.dragged() || position_response.changed()
      {
         gradient_position = gradient_position.clamp(MIN_GRADIENT_POSITION, max_gradient_position);
         me.gradient_offset.store(gradient_position);
         me.is_first_gradient_frame = true;
         {
            let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
            let mut settings_lock = settings.lock();
            settings_lock.gradient_offset = gradient_position as f64;
            match settings_lock.write_settings() {
               | Ok(_) => (),
               | Err(e) => eprintln!("Error saving gradient offset to settings: {}", e),
            }
         }
      }

      ui.separator();

      ui.label("Vertical Scale:");
      let scaling_response = ui
         .add_sized(
            egui::Vec2::new(100.0, 30.0),
            egui::DragValue::new(&mut vertical_scale)
               .range(1.0..=50.0)
               .speed(0.5)
               .max_decimals(1),
         )
         .on_hover_text("Vertical scaling for gradient");
      if scaling_response.dragged() || scaling_response.changed() {
         me.vertical_scale.store(vertical_scale);
         me.is_first_gradient_frame = true;
         {
            let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
            let mut settings_lock = settings.lock();
            settings_lock.vertical_exaggeration = vertical_scale;
            _ = settings_lock.write_settings();
         }
      }
   });
   ui.horizontal(|ui| {
      ui.label("Flat Gradient (%):");
      let flat_gradient_response = ui
         .add_sized(
            egui::Vec2::new(100.0, 30.0),
            egui::DragValue::new(&mut flat_gradient)
               .suffix("%")
               .range(0.1..=2.5)
               .speed(0.1)
               .max_decimals(1),
         )
         .on_hover_text("The gradient considered to be 'flat', e.g if 0.5 then -0.5 to 0.5 is flat");
      if flat_gradient_response.dragged() || flat_gradient_response.changed() {
         me.gradient_flat.store(flat_gradient);
         me.is_first_gradient_frame = true;
         {
            let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
            let mut settings_lock = settings.lock();
            settings_lock.flat_gradient_percentage = flat_gradient;
            _ = settings_lock.write_settings();
         }
      }

      ui.label("Medium Gradient (%):");
      let medium_gradient_response = ui
         .add_sized(
            egui::Vec2::new(100.0, 30.0),
            egui::DragValue::new(&mut medium_gradient)
               .suffix("%")
               .range(1.0..=16.0)
               .speed(0.1)
               .max_decimals(1),
         )
         .on_hover_text("The gradient considered to be 'medium'")
         .on_hover_text("If flat to medium then gradient color is a shade of yellow; if medium to extreme then red.");
      if medium_gradient_response.dragged() || medium_gradient_response.changed() {
         me.gradient_medium.store(medium_gradient);
         me.is_first_gradient_frame = true;
         {
            let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
            let mut settings_lock = settings.lock();
            settings_lock.medium_gradient_percentage = medium_gradient;
            _ = settings_lock.write_settings();
         }
      }

      ui.label("Extreme Gradient (%):");
      let extreme_gradient_response = ui
         .add_sized(
            egui::Vec2::new(100.0, 30.0),
            egui::DragValue::new(&mut extreme_gradient)
               .range(10.0..=100.0)
               .speed(0.5)
               .max_decimals(1),
         )
         .on_hover_text("The gradient considered to be 'extreme' (black), e.g if > 16 then gradient color is black");
      if extreme_gradient_response.dragged() || extreme_gradient_response.changed() {
         me.gradient_extreme.store(extreme_gradient);
         me.is_first_gradient_frame = true;
         {
            let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
            let mut settings_lock = settings.lock();
            settings_lock.extreme_gradient_percentage = extreme_gradient;
            _ = settings_lock.write_settings();
         }
      }
   });
}

fn render_current_gradient(me: &mut GPXAssistUI, ui: &mut egui::Ui)
//------------------------------------------------------
{
   ui.vertical(|ui| {
      gradient_options(me, ui);
      // ui.centered_and_justified(|ui|
      // {
      if let Some(texture) = &me.gradient_texture {
         ui.add(
            Image::new(texture)
               .maintain_aspect_ratio(true)
               .fit_to_original_size(1.0)
               .shrink_to_fit(),
         );
      }
   });
}

/// Load an embedded PNG image as ColorImage
fn load_embedded_png(asset_name: &str) -> Result<ColorImage, String>
//--------------------------------------------------------------------
{
   let png_data = super::ui::ASSETS_DIR
      .get_file(asset_name)
      .ok_or_else(|| format!("Failed to find embedded asset: {}", asset_name))?
      .contents();

   let img = image::load_from_memory(png_data).map_err(|e| format!("Failed to decode PNG: {}", e))?;

   let rgba = img.to_rgba8();
   let size = [rgba.width() as usize, rgba.height() as usize];
   let pixels = rgba.into_raw();

   Ok(ColorImage::from_rgba_unmultiplied(size, &pixels))
}

pub fn color_from_gradient(gradient_pct: f64, flat_gradient: f64, medium_gradient: f64, extreme_gradient: f64, extreme_start: f64) -> tiny_skia::Color
//--------------------------------------------------------------------
{
   if gradient_pct < -flat_gradient.abs() {
      // Downhill: light blue to dark blue
      // let t = ((gradient_pct - flat_gradient.abs()) / extreme_gradient.abs()).min(1.0);
      let t = ((-flat_gradient.abs() - gradient_pct) / extreme_gradient.abs())
         .abs()
         .min(1.0);
      let b = (255.0) as u8;
      let g = (216.0 * (1.0 - t)) as u8;
      let r = (173.0 * (1.0 - t)) as u8;
      // println!(" (downhill {}<{}:  {} {} {})",gradient_pct,-flat_gradient.abs(), r, g, b);
      // tiny_skia::Color::from_rgba8(r, g, b, 255)
      tiny_skia::Color::from_rgba8(b, g, r, 255)
   } else if gradient_pct > flat_gradient.abs() {
      if gradient_pct >= extreme_gradient.abs() {
         // println!(" (extreme {}>={}:  {} {} {})",gradient_pct, extreme_gradient.abs(), 0.0, 0.0, 0.0);
         tiny_skia::Color::from_rgba8(0, 0, 0, 255)
      } else if gradient_pct >= medium_gradient.abs() {
         //gradient of red
         let t = ((gradient_pct - medium_gradient.abs()) / extreme_gradient.abs()).min(1.0);
         let b = if gradient_pct > extreme_start { 0 } else { 8 };
         let g = 0;
         let r = (220.0 * (1.0 - t)) as u8; //((255.0 * (1.0 - t)) as u8);
         // println!(" (steep {}>={}:  {} {} {})",gradient_pct, medium_gradient.abs(), r, g, b);
         tiny_skia::Color::from_rgba8(b, g, r, 255)
      } else {
         // Medium Uphill: shades of yellow
         let t = ((gradient_pct - flat_gradient.abs()) / extreme_gradient.abs()).min(1.0);
         let b = 0 as u8;
         let g = (155.0 * (1.0 - t)) as u8;
         let r = (255.0 - (33.0 * (1.0 - t))) as u8;
         // println!(" (medium {} <= {} <= {}:  {} {} {})", flat_gradient.abs(), gradient_pct, medium_gradient.abs(), r, g, b);
         tiny_skia::Color::from_rgba8(b, g, r, 255)
      }
   } else
   //flat
   {
      // tiny_skia::Color::from_rgba8(50, 200, 50, 255)
      let t = ((flat_gradient.abs() - gradient_pct) / extreme_gradient.abs())
         .abs()
         .min(1.0);
      let b = 0;
      let g = (255.0 * (1.0 - t)) as u8;
      let r = 0;
      // println!(" (flat {} <= {} <= {}:  {} {} {})", -flat_gradient.abs(), gradient_pct, flat_gradient.abs(), r, g, b);
      // tiny_skia::Color::from_rgba8(r, g, b, 255)
      tiny_skia::Color::from_rgba8(b, g, r, 255)
   }
}

fn display_open_gpx(ui: &mut egui::Ui, open_img: Option<&(TextureHandle, [f32; 2])>)
//------------------------------------
{
   ui.vertical(|ui|
   {
      ui.horizontal(|ui|
      {
         ui.add(egui::Label::new(
         egui::RichText::new("GPX/FIT file not loaded.")
            .strong()
            .color(egui::Color32::RED),
         ));
         ui.add_space(10.0);
         match open_img
         {
            | Some((texture, size)) =>
            {
               ui.add(egui::Label::new("Click the Open toolbar button "));
               ui.add(egui::Image::new(texture)
                  .alt_text("Open")
                  .bg_fill(egui::Color32::from_rgb(232, 227, 209))
                  .fit_to_exact_size((*size).into()));
               ui.add(egui::Label::new(" to load a .gpx file."));
            }
            | None =>
            {
               ui.add(egui::Label::new("Click the Open toolbar button to load a .gpx or .fit file."));
            }
         };
      });

      ui.add(egui::Label::new(
         egui::RichText::new(r#"In order to display gradient, look ahead altitude data is needed, therefore either a .gpx or .fit file
 with location and altitude information must be loaded to use the gradient option."#)
            .color(egui::Color32::YELLOW),
      ));

      let home = match get_home_directory()
      {
         | Some(dir) => dir.join("680009BE52697B069/FITFiles/").as_os_str().display().to_string(),
         | None => "".to_string(),
      };
      let message = format!(r#"If you previously rode the route, then a .fit file can be obtained from the TPV
 Document directory under your TPV ID sub-directory (a long unique TPV identifier) e.g {}.

If you upload to Strava then clicking the activity and then clicking the "3 dots" menu options
 on the left lets you download either a .gpx or .fit file"#, home);
      ui.add(egui::Label::new(
         egui::RichText::new(message).color(egui::Color32::YELLOW),
      ));
   });
}

fn display_invalid_broadcast_directory(ui: &mut egui::Ui, is_aged: bool, delta: i64)
//----------------------------------------------------
{
   let broadcast_file = match get_broadcast_file() {
      | Some(dir) => dir,
      | None => PathBuf::from(""),
   };
   let err_color: Color32;
   let errmsg = if broadcast_file.is_file() && is_aged {
      err_color = Color32::YELLOW;
      format!(
         "The broadcast file {:?} has not been updated recently enough (try pedalling for more than {} metres).",
         broadcast_file, delta
      )
      .to_string()
   } else {
      err_color = Color32::RED;
      format!("Could not find a valid TrainingPeaks Virtual broadcast file at {:#?}.", broadcast_file).to_string()
   };

   // Load embedded PNG images - unwrap is safe since assets are embedded at compile time
   let color_img_1 = load_embedded_png("menu-1.png").expect("menu-1.png should be embedded");
   let texture_1 = ui
      .ctx()
      .load_texture("menu_1", color_img_1, Default::default());
   let image_1 = Image::new(&texture_1)
      .maintain_aspect_ratio(true)
      .fit_to_fraction(Vec2 { x: 0.1, y: 0.5 })
      .shrink_to_fit();

   let color_img_2 = load_embedded_png("menu-2.png").expect("menu-2.png should be embedded");
   let texture_2 = ui
      .ctx()
      .load_texture("menu_2", color_img_2, Default::default());
   let image_2 = Image::new(&texture_2)
      .max_size(Vec2 { x: 115.0, y: 142.0 })
      .shrink_to_fit();

   let color_img_3 = load_embedded_png("menu-3.png").expect("menu-3.png should be embedded");
   let texture_3 = ui
      .ctx()
      .load_texture("menu_3", color_img_3, Default::default());
   let image_3 = Image::new(&texture_3)
      .maintain_aspect_ratio(true)
      .fit_to_fraction(Vec2 { x: 0.1, y: 0.5 })
      .shrink_to_fit();
   ui.vertical(|ui|
   {
      ui.add_space(16.0);
      ui.add(egui::Label::new( egui::RichText::new(errmsg)
               .strong().color(err_color)));
      ui.add_space(32.0);
      ui.separator();
      ui.add(egui::Label::new( egui::RichText::new("Try opening settings in TrainingPeaks Virtual")
               .color(egui::Color32::GREEN)));
      ui.add_space(5.0);
      ui.add(image_1);
      ui.add_space(10.0);
      ui.add(egui::Label::new( egui::RichText::new("Then select Broadcast Settings")
               .color(egui::Color32::GREEN)));
      ui.add_space(5.0);
      ui.add(image_2);
      ui.add(egui::Label::new( egui::RichText::new("Finally enable Broadcasting to file, and click the Test button which should create test files")
               .color(egui::Color32::GREEN)));
      ui.add_space(5.0);
      ui.add(image_3);
      ui.add_space(10.0);
      let settings_dir = Settings::get_settings_path().unwrap_or(PathBuf::from("."));
      let errmsg = format!(r#"If the broadcast file location is still incorrect, use the path below the "Save to Local File" as shown in the image above either in the settings dialog or modify the settings file {:#?} directly."#, settings_dir);
      ui.add(egui::Label::new(
         egui::RichText::new(errmsg)
                  .color(egui::Color32::LIGHT_YELLOW)));
   });
}

fn open_file_dialog(ctx: &Context, sender: Sender<(Vec<TrackPoint>, String, String)>)
//--------------------------------------------------------------------------
{
   let pick_dir: PathBuf;
   {
      let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
      pick_dir = settings.lock().get_last_directorybuf();
   }
   let dialog_future = rfd::AsyncFileDialog::new()
      .add_filter("GPX/FIT files", &["gpx", "fit"])
      .set_directory(pick_dir)
      .set_title("Open GPX or FIT File")
      .pick_file();
   let ctxx = ctx.clone();
   start_dialog_thread(async move
   {
      let file_info = dialog_future.await;
      if let Some(fileinfo) = file_info 
      {
         let path = fileinfo.path();
         match path.parent() {
            | Some(d) => {
               let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
               settings.lock().set_last_directorybuf(&d.to_path_buf());
            }
            | None => (),
         };
         let file_path_disp = &path.display().to_string();
         let ext = match path.extension()
         {
            | Some(e) => e.to_string_lossy().to_lowercase(),
            | None => "".to_string(),
         };
         let track_result: Result<Vec<TrackPoint>, Box<dyn std::error::Error>> = match ext.as_str()
         {
            | "fit" =>
            {
               println!("Processing FIT file: {:?}", fileinfo.path());
               process_fit(file_path_disp.clone().as_str())
            }
            | "gpx" =>
            {
               process_gpx(file_path_disp.clone().as_str())
            }
            | _ => Err(format!("Unsupported file type: {}", ext).into())
         };
         let errmsg: String;
         let track_data = match track_result
         {
            | Ok(trackdata) =>
            {
               println!("Successfully processed {} points.", trackdata.len());
               errmsg = "".into();
               trackdata
            }
            | Err(e) =>
            {
               errmsg = format!("Error processing file {:?}: {}", fileinfo.path(), e);
               Vec::new()
            }
         };
         let _ = sender.send((track_data, file_path_disp.clone(), errmsg));
         // let _ = sender.send(String::from_utf8_lossy(&text).to_string());
         ctxx.request_repaint();
      }
   });
}

fn start_dialog_thread<F: Future<Output = ()> + Send + 'static>(f: F) 
//---------------------------------------------------------------------
{
   std::thread::spawn(move || futures::executor::block_on(f));
}

fn set_style(ctx: &Context)
//--------------------
{
   let mut style: egui::Style = (*ctx.style()).clone();
   style.visuals.window_fill = egui::Color32::from_rgb(30, 30, 30);
   style.visuals.image_loading_spinners = true;
   style.text_styles = [
      (egui::TextStyle::Heading, egui::FontId::new(30.0, egui::FontFamily::Proportional)),
      (egui::TextStyle::Body, egui::FontId::new(20.0, egui::FontFamily::Proportional)),
      (egui::TextStyle::Monospace, egui::FontId::new(20.0, egui::FontFamily::Monospace)),
      (egui::TextStyle::Button, egui::FontId::new(20.0, egui::FontFamily::Proportional)),
      (egui::TextStyle::Small, egui::FontId::new(15.0, egui::FontFamily::Proportional)),
   ]
   .into();
   ctx.set_style(style);
}

pub fn streetview(api_key: &str, previous_position: &WGS84Position, position: &WGS84Position, width: f32, height: f32, fov: i64,
   use_heading: bool, _is_debug: bool,
) -> Result<ColorImage, String>
//--------------------------
{
   // let fov = 90; // Field of view (0-120 degrees)
   let current_latitude = position.latitude;
   let current_longitude = position.longitude;
   let heading = calculate_bearing(previous_position.latitude, previous_position.longitude, current_latitude, current_longitude);

   let pitch = 0; // Up/down angle (-90 to 90 degrees)
   let w = width as u32; // width.min(640.0).round() as u32;
   let h = height as u32; // height.min(640.0).round() as u32;

   // Construct the Google Street View API URL
   let url: String;
   if use_heading {
      url = format!(
         "https://maps.googleapis.com/maps/api/streetview?size={w}x{h}&location={current_latitude},{current_longitude}&fov={fov}&heading={heading}&pitch={pitch}&key={api_key}"
      );
   } else {
      url = format!(
         "https://maps.googleapis.com/maps/api/streetview?size={w}x{h}&location={current_latitude},{current_longitude}&fov={fov}&pitch={pitch}&key={api_key}"
      );
   }
   println!("Fetching Street View from: {}", url);

   // Fetch and load the image
   fetch_image_from_url(&url)
}

/// Draw summit info text on the top left of the gradient image
/// Shows "Distance to [marker] X.XXkm" where [marker] is a small diamond symbol
pub(crate) fn draw_summit_info(pixmap: &mut tiny_skia::Pixmap, distance_to_summit: f64, padding: f32)
//---------------------------------------------------------------------------------------------------------------
{
   use fontdue::{Font, FontSettings};

   const FONT_DATA: &[u8] = include_bytes!("../../assets/InterVariable.ttf"); //("../../assets/Roboto-Regular.ttf");

   let font = match Font::from_bytes(FONT_DATA, FontSettings::default()) {
      | Ok(f) => f,
      | Err(_) => return,
   };

   let font_size = 28.0;
   let baseline_y = padding - 10.0; // Baseline position above the plot area
   let text_x = padding;

   let pixmap_width = pixmap.width();
   let pixmap_height = pixmap.height();
   let mut x_offset = text_x as f64;

   // Draw small diamond marker inline with text
   let marker_x = x_offset as f32 + 8.0;
   let marker_y = baseline_y - font_size / 3.0; // Center marker vertically relative to baseline
   let diamond_size = 6.0;

   let mut path_builder = tiny_skia::PathBuilder::new();
   path_builder.move_to(marker_x, marker_y - diamond_size); // Top
   path_builder.line_to(marker_x + diamond_size * 0.7, marker_y); // Right
   path_builder.line_to(marker_x, marker_y + diamond_size); // Bottom
   path_builder.line_to(marker_x - diamond_size * 0.7, marker_y); // Left
   path_builder.close();

   if let Some(path) = path_builder.finish() {
      let mut paint = tiny_skia::Paint::default();
      paint.set_color(tiny_skia::Color::from_rgba8(255, 215, 0, 255)); // Gold color
      paint.anti_alias = true;
      pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);

      // Draw outline
      let stroke = tiny_skia::Stroke {
         width: 1.0,
         ..Default::default()
      };
      paint.set_color(tiny_skia::Color::from_rgba8(0, 0, 0, 255));
      pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
   }

   // Draw distance value after the marker
   x_offset = marker_x as f64 + diamond_size as f64 + 8.0;
   let distance_km = distance_to_summit / 1000.0;
   let distance_text = format!(" - {:.2}km", distance_km);

   for ch in distance_text.chars() {
      let (metrics, bitmap) = font.rasterize(ch, font_size);

      if metrics.width > 0 {
         // Calculate y position using baseline alignment
         // metrics.ymin is the offset from baseline to the bottom of the glyph (negative for descenders)
         // The glyph should be positioned so its baseline aligns with baseline_y
         let glyph_top_y = baseline_y - metrics.height as f32 - metrics.ymin as f32;

         for (py, row) in bitmap.chunks(metrics.width).enumerate() {
            for (px, &alpha) in row.iter().enumerate() {
               if alpha > 0 {
                  let pixel_x = (x_offset + px as f64) as u32;
                  let pixel_y = (glyph_top_y + py as f32) as u32;

                  if pixel_x < pixmap_width && pixel_y < pixmap_height {
                     let color = tiny_skia::Color::from_rgba8(0, 0, 0, alpha);
                     pixmap.pixels_mut()[(pixel_y * pixmap_width + pixel_x) as usize] = color.premultiply().to_color_u8();
                  }
               }
            }
         }
      }
      x_offset += metrics.advance_width as f64;
   }
}

/// Helper function to draw distance labels on the gradient profile
pub(crate) fn draw_distance_labels( pixmap: &mut tiny_skia::Pixmap, segment_start_distance: i64, segment_end_distance: i64,
   label_width: i64, padding: f32, plot_width: f32, plot_height: f32,
)
//---------------------------------------------------------------------------------------------------------------
{
   use fontdue::{Font, FontSettings};

   // Embedded font data (using a simple fallback)
   // const FONT_DATA: &[u8] = include_bytes!("../../assets/Roboto-Regular.ttf");
   const FONT_DATA: &[u8] = include_bytes!("../../assets/InterVariable.ttf"); //("../../assets/Roboto-Regular.ttf");

   let font = match Font::from_bytes(FONT_DATA, FontSettings::default()) {
      | Ok(f) => f,
      | Err(_) => return, // Skip labels if font fails to load
   };

   let font_size = 18.0;
   let baseline_y = padding + plot_height + 40.0; // Baseline position below the plot
   let distance_range = segment_end_distance - segment_start_distance;

   // Calculate number of labels based on label_width
   let num_labels = (distance_range as f64 / label_width as f64).ceil() as usize + 1;

   let pixmap_width = pixmap.width();
   let pixmap_height = pixmap.height();

   for i in 0..num_labels
   {
      if i % 2 != 0 
      {
         continue; // Skip every other label for clarity
      }
      let distance_at_label = segment_start_distance + (i as i64 * label_width as i64);
      if distance_at_label > segment_end_distance
      {
         break;
      }

      // Convert distance to km for display
      let distance_km = distance_at_label as f64 / 1000.0;
      let label_text = format!("{:.1}", distance_km);

      // Calculate x position for this label
      let x = padding as f64 + ((distance_at_label - segment_start_distance) as f64/ distance_range as f64) * plot_width as f64;

      // Render the text
      let mut x_offset = x;

      for ch in label_text.chars() {
         let (metrics, bitmap) = font.rasterize(ch, font_size);

         if metrics.width > 0 {
            // Calculate y position using baseline alignment
            let glyph_top_y = baseline_y - metrics.height as f32 - metrics.ymin as f32;

            // Draw each pixel of the character
            for (py, row) in bitmap.chunks(metrics.width).enumerate() {
               for (px, &alpha) in row.iter().enumerate() {
                  if alpha > 0 {
                     let pixel_x = (x_offset + px as f64) as u32;
                     let pixel_y = (glyph_top_y + py as f32) as u32;

                     if pixel_x < pixmap_width && pixel_y < pixmap_height {
                        let color = tiny_skia::Color::from_rgba8(0, 0, 0, alpha);
                        pixmap.pixels_mut()[(pixel_y * pixmap_width + pixel_x) as usize] = color.premultiply().to_color_u8();
                     }
                  }
               }
            }
         }
         x_offset += metrics.advance_width as f64;
      }

      // Draw tick mark
      let tick_x = x as f32;
      let tick_top = padding + plot_height;
      let tick_bottom = tick_top + 5.0;

      let mut path_builder = tiny_skia::PathBuilder::new();
      path_builder.move_to(tick_x, tick_top);
      path_builder.line_to(tick_x, tick_bottom);

      if let Some(path) = path_builder.finish() {
         let mut paint = tiny_skia::Paint::default();
         paint.set_color(tiny_skia::Color::from_rgba8(0, 0, 0, 255));
         paint.anti_alias = true;
         let stroke = tiny_skia::Stroke {
            width: 2.0,
            ..Default::default()
         };
         pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
      }
   }
}

pub(crate) fn pixmap_to_image(pixmap: &tiny_skia::Pixmap, pixmap_width: u32, pixmap_height: u32) -> ColorImage
//-----------------------------------------------
{
   let pixels = pixmap.data();
   let mut rgba_pixels = Vec::with_capacity((pixmap_width * pixmap_height * 4) as usize);

   for chunk in pixels.chunks_exact(4) {
      let r = chunk[2]; // tiny_skia is BGRA
      let g = chunk[1];
      let b = chunk[0];
      let a = chunk[3];

      rgba_pixels.push(r);
      rgba_pixels.push(g);
      rgba_pixels.push(b);
      rgba_pixels.push(a);
   }
   ColorImage::from_rgba_unmultiplied([pixmap_width as usize, pixmap_height as usize], &rgba_pixels)
}

/// Helper function to fetch an image from a URL
fn fetch_image_from_url(url: &str) -> Result<ColorImage, String>
//------------------------------------------------------------------
{
   // Fetch the image using reqwest
   let response = reqwest::blocking::get(url).map_err(|e| format!("Failed to fetch image: {}", e))?;

   // Check response status
   let status = response.status();
   if !status.is_success() {
      return Err(format!("HTTP error: {} - Check if location has Street View coverage", status));
   }

   let bytes = response
      .bytes()
      .map_err(|e| format!("Failed to read response: {}", e))?;

   // Check if we got actual image data
   if bytes.len() < 100 {
      return Err("Received suspiciously small response - location may not have Street View coverage".to_string());
   }

   // Decode the image
   let img = image::load_from_memory(&bytes).map_err(|e| format!("Failed to decode image: {}", e))?;

   let rgba = img.to_rgba8();
   let size = [rgba.width() as usize, rgba.height() as usize];
   let pixels = rgba.into_raw();

   println!("Decoded image: {}x{}, {} bytes", size[0], size[1], pixels.len());

   Ok(ColorImage::from_rgba_unmultiplied(size, &pixels))
}

pub fn get_broadcast_directory() -> Option<PathBuf>
//---------------------------------------------
{
   if cfg!(target_os = "macos") {
      // ~/TPVirtual/Broadcast/focus.json
      match dirs::home_dir() {
         | Some(dir) => Some(dir.join("TPVirtual").join("Broadcast").clone()),
         | None => None,
      }
   } else {
      match dirs::document_dir() {
         | Some(dir) => Some(dir.join("TPVirtual").join("Broadcast").clone()),
         | None => None,
      }
   }
}

pub fn get_home_directory() -> Option<PathBuf>
//---------------------------------------------
{
   if cfg!(target_os = "macos") {
      // ~/TPVirtual/Broadcast/focus.json
      match dirs::home_dir() {
         | Some(dir) => Some(dir.join("TPVirtual").clone()),
         | None => None,
      }
   } else {
      match dirs::document_dir() {
         | Some(dir) => Some(dir.join("TPVirtual").clone()),
         | None => None,
      }
   }
}


pub fn get_broadcast_file() -> Option<PathBuf>
//---------------------------------------------
{
   match get_broadcast_directory() {
      | Some(dir) => Some(dir.join("focus.json")).clone(),
      | None => None,
   }
}

/// Returns the distance in meters from the broadcast focus.json file.
/// -1 indicates an error parsing the file after parse_retries attempts.
pub(crate) fn read_rider_data(parse_retries: i64, retry_duration: Duration) -> Option<RiderDataJSON>
//--------------------------------------
{
   let broadcast_file = match get_broadcast_file() {
      | Some(f) => {
         if !f.exists() {
            return None;
         } else {
            f
         }
      }
      | None => {
         return None;
      }
   };

   for _ in 0..parse_retries {
      let rider_json_data = match std::fs::read_to_string(&broadcast_file) {
         | Ok(data) => {
            //.ok()?.trim().to_string(); //[{"name":"xxx"....}]
            let s = data.trim().to_string();
            if s.is_empty() {
               return None;
            }
            s
         }
         | Err(_) => {
            return None;
         }
      };

      // The data as read from disk has 3 binary characters at the start which cause JSON parsing to fail.
      // Turns out its a UTF-8 BOM (Byte Order Mark) (https://en.wikipedia.org/wiki/Byte_order_mark)
      // which Rusts standard library does not strip automatically.
      let mut pch = rider_json_data.find('[');
      if pch.is_none() {
         pch = rider_json_data.find('{');
         if pch.is_none() {
            return None;
         }
      }

      let p = pch.unwrap_or(0);
      let rider_json_data = if p > 0 { rider_json_data[p..].to_string() } else { rider_json_data };

      // Handle (invalid) unnamed JSON array [{"name":"xxx"....}] (should be for eg { "riders": [ {"name":"xxx"....}] }
      // (must have come from some Microsoft JSON serializer).
      // let rider_json = if rider_json_data.starts_with(r#"["#) && rider_json_data.ends_with(r#"]"#)
      // {
      //    rider_json_data[1..rider_json_data.len()-1].to_string() // remove [ and ]
      // }
      // else
      // {
      //    rider_json_data
      // };
      // println!("Read rider JSON: {}", rider_json_data);
      let rider_json = rider_json_data
         .strip_prefix('[')
         .and_then(|s| s.strip_suffix(']'))
         .unwrap_or(&rider_json_data)
         .to_string()
         .trim()
         .to_string();

      // println!("Process rider JSON: {}", rider_json);

      if let Ok(rider_data) = RiderDataJSON::from_json(&rider_json) {
         return Some(rider_data);
      }
      std::thread::sleep(retry_duration);
   }
   None
}
