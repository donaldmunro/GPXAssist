use std::{future::Future, path::PathBuf, sync::{Arc, atomic::Ordering, mpsc::Sender}, time::Duration};

use eframe::egui::{self, Color32, ColorImage, Context, Frame, Image, Vec2};
use walkers::{lon_lat, Map};
use tiny_skia::{Pixmap, Paint, PathBuilder, Stroke, Transform, FillRule};

use crate::{components::DirectionalArrow, data::{RiderData, RiderDataJSON}, gpx::{TrackPoint, find_closest_point, process_gpx}};
use eframe::emath::Numeric;
use crate::SETTINGS;
use crate::settings::Settings;

use super::ui::{GPXAssistUI, ViewMode};

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
            if !tt.0.is_empty()
            {
               let (trackdata, filepath) = tt;
               self.gpx_file = Some(PathBuf::from(&filepath));
               self.total_distance = trackdata.last().map_or(0.0, |p| p.distance);
               self.current_distance = 0.0;
               self.updated_distance.store(0.0);
               self.is_first_map_frame = true;
               // self.first_map_count = 3;
               self.is_first_street_frame = true;
               self.current_position = trackdata.first().copied(); //.map(|p| *p);
               self.previous_position = self.current_position;
               self.gpx_track = Arc::new(trackdata);
               self.current_mode = Arc::new(crossbeam::atomic::AtomicCell::new(ViewMode::Map));
               self.is_simulating.store(false, Ordering::Relaxed);
               match PathBuf::from(&filepath).file_name()
               {
                  | Some(name) =>
                  {
                     let title = "GPXAssist: ".to_string() + &name.to_string_lossy();
                     ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                  },
                  | None => ()
               }
               self.is_running.store(true, Ordering::Relaxed);
               let current_mode = self.current_mode.clone();
               let updated_distance = self.updated_distance.clone();
               let requested_delta = self.requested_delta.clone();
               let gradient_delta = self.gradient_delta.clone();
               let rider_data = self.rider_data.clone();
               let total_distance = self.total_distance;
               let is_running = self.is_running.clone();
               let track = self.gpx_track.clone();
               let ctxx = ctx.clone();
               self.is_first_map_frame = false;
               self.is_first_street_frame = false;
               self.is_first_gradient_frame = false;
               let mut gradient_length: f64;
               let mut gradient_offset: f64;
               let mut flat_gradient: f64;
               let mut extreme_gradient: f64;
               let mut vertical_exaggeration: f64;
               {
                  let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
                  let settings_lock = settings.lock();
                  gradient_length = settings_lock.gradient_length;
                  if gradient_length <= 0.0 || gradient_length >= 20000.0 { gradient_length = 3000.0; }
                  gradient_offset = settings_lock.gradient_offset;
                  self.gradient_offset.store(gradient_offset);
                  if gradient_offset < 0.0 || gradient_offset >= gradient_length { gradient_offset = 100.0 }
                  self.gradient_length.store(gradient_length);
                  flat_gradient = settings_lock.flat_gradient_percentage;
                  if flat_gradient < 0.0 || flat_gradient >= 5.0 { flat_gradient = 0.3; }
                  self.gradient_flat.store(flat_gradient);
                  extreme_gradient = settings_lock.extreme_gradient_percentage;
                  if extreme_gradient < 5.0 || extreme_gradient > 100.0 { extreme_gradient = 16.0; }
                  self.gradient_extreme.store(extreme_gradient);
                  vertical_exaggeration = settings_lock.vertical_exaggeration;
                  if vertical_exaggeration < 1.0 || vertical_exaggeration > 50.0 { vertical_exaggeration = 10.0; }
                  self.vertical_scale.store(vertical_exaggeration);
               }
               std::thread::spawn(move ||
               {
                  GPXAssistUI::update_distance_thread(ctxx, updated_distance, track, requested_delta, gradient_delta, rider_data, total_distance, current_mode, is_running);
               });
            }
            else
            {
               self.toast_manager.error("The selected GPX file contains no track points or could not be processed.", None);
            }
         }

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

            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);

            if let Some((texture, size)) = self.textures.get("open")
               && ui.add(egui::Button::image(egui::Image::new(texture)
                     .alt_text("Open")
                     .bg_fill(egui::Color32::from_rgb(232, 227, 209))
                     .fit_to_exact_size((*size).into()))).clicked()
            {
               let sender = self.open_dialog_channel.0.clone();
               open_file_dialog(ui.ctx(), sender);
            }

            if self.gpx_file.is_some() && self.total_distance > 0.0
            {
               let mut dist: f64 = self.requested_delta.load();
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
               .on_hover_text("The distance in metres to travel before updating the current view. Drag with mouse or enter a value.");
               if distance_response.dragged() || distance_response.changed()
               {
                  self.requested_delta.store(dist);
                  println!("Requested Distance Delta set to {:.2} meters", dist);
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

               let mut speed: f64 = self.simulated_speed.load();
               ui.label(egui::RichText::new("Speed:").color(egui::Color32::YELLOW).strong());
               let speed_response = ui.add_sized(
                  egui::Vec2::new(60.0, 30.0), // Fixed size: width = 60, height = 30
                  egui::DragValue::new(&mut speed)
                     .range(0.0..=200.0)
                     .min_decimals(0)
                     .max_decimals(0)
                     .speed(1.0)
                     .clamp_existing_to_range(true))
               .on_hover_text("The speed in km/h when simulating. Drag with mouse or enter a value.");
               if speed_response.dragged() || speed_response.changed()
               {
                  self.simulated_speed.store(speed);
                  println!("Simulated speed set to {:.2} meters", speed);
               }

               if self.is_simulating.load(Ordering::Relaxed) && ! self.is_running.load(Ordering::Relaxed)
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
                     self.is_running.store(true, Ordering::Relaxed);
                  }
               }
               else if  ! self.is_simulating.load(Ordering::Relaxed)
                        && let Some((texture, size)) = self.textures.get("test-on")
                        && self.total_distance > 0.0
                        && ui.add(egui::Button::image(egui::Image::new(texture)
                              .alt_text("Test")
                              .bg_fill(egui::Color32::from_rgb(232, 227, 209))
                              .fit_to_exact_size((*size).into())).selected(false))
                              .on_hover_text("Start simulating movement along the GPX track at 45km/h.")
               .clicked()
               {
                  self.is_simulating.store(true, Ordering::Relaxed);
                  self.is_running.store(false, Ordering::Relaxed);
                  let updated_distance = self.updated_distance.clone();
                  let rider_data = self.rider_data.clone();
                  let requested_delta = self.requested_delta.clone();
                  let gradient_delta = self.gradient_delta.clone();
                  let simulated_speed = self.simulated_speed.clone();
                  let total_distance = self.total_distance;
                  let is_running = self.is_running.clone();
                  let is_sim_running = self.is_simulating.clone();
                  let current_mode = self.current_mode.clone();
                  let track = self.gpx_track.clone();
                  let ctxx = ctx.clone();
                  std::thread::spawn(move ||
                  {
                     GPXAssistUI::simulate_movement_thread(ctxx, updated_distance, track, requested_delta, gradient_delta, simulated_speed, rider_data, total_distance,
                        current_mode, is_sim_running, is_running);
                  });
               }
            }
         })
      } );

      egui::CentralPanel::default()
      .show(ctx, |ui|
      {
         let (exists_broadcast_file, aged_broadcast_file) = self.check_broadcast_file();
         let broadcast_file = get_broadcast_file();
         let current_mode = self.current_mode.load();
         if current_mode == ViewMode::NA || self.gpx_file.is_none() || self.total_distance == 0.0
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
         else if  ! self.is_simulating.load(Ordering::Relaxed) && (broadcast_file.is_none() || !broadcast_file.as_ref().unwrap().is_file() ||
                  ! exists_broadcast_file || aged_broadcast_file)
         {
            let delta = self.requested_delta.load();
            display_invalid_broadcast_directory(ui, aged_broadcast_file, delta);
         }
         else
         {
            let rider_data = self.rider_data.load();
            let updated_distance = self.updated_distance.load();
            let requested_delta = self.requested_delta.load();
            let is_update = (self.updated_distance.load() - self.current_distance) >= requested_delta;
            let gradient_delta = self.gradient_delta.load();

            if current_mode == ViewMode::Map //&& is_update
                  && let Some(current_position) = self.current_position
                  && let (Some(tiles), Some(memory)) = (&mut self.tiles, &mut self.map_memory)
                  && let (Some(position), _) = find_closest_point(&self.gpx_track, self.updated_distance.load())
            {
               let point = lon_lat(position.point.lon, position.point.lat);
               ui.add(
                  Map::new(Some(tiles), memory, point)
                     .with_plugin(DirectionalArrow
                     {
                        current_position: lon_lat(position.point.lon, position.point.lat),
                        heading: position.heading,
                        wind_angle: rider_data.wind_angle,
                        wind_speed: rider_data.wind_speed.to_f64() / 1000.0 // wind speed is in mm/s so convert to m/s
                     })
               );
               self.previous_position = self.current_position;
               self.current_position = Some(position);
               self.current_distance = updated_distance;
            }
            else  if current_mode == ViewMode::StreetView
            {
               if self.encrypted_api_key.is_none()
               {
                  display_streetview_info(ui);
               }
               else  if self.gpx_file.is_some() && (is_update || self.is_first_street_frame)
               {
                  display_streetview(self, ctx, ui, requested_delta, updated_distance);
               }
               else if self.gpx_file.is_some()
                  && let Some(texture) = &self.streetview_texture
                  // && let Some(current_position) = self.current_position
                  // && let Some(position) = find_closest_point(&self.gpx_track, updated_distance)
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
            } // self.current_mode == ViewMode::StreetView
            else if  current_mode == ViewMode::Gradient
            {
               let is_gradient_update = ! is_update && ( (gradient_delta < requested_delta) && (updated_distance - self.gradient_distance) >= gradient_delta );
               // println!("Gradient: {gradient_delta} < {requested_delta} | {updated_distance} {} {} {} {}", self.gradient_distance, updated_distance, self.current_distance, self.gradient_distance);
               if (is_update || self.is_first_gradient_frame) &&
                  let (Some(position), _) = find_closest_point(&self.gpx_track, updated_distance)
               {
                  // println!("Gradient Regen {:?} {}", position, updated_distance);
                  let available_size = ui.available_size();
                  let mut errmsg = String::new();
                  let gradient_image = match new_gradient_image(self, &position, available_size.x, available_size.y, 1000.0)
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
                  self.previous_position = self.current_position;
                  self.current_position = Some(position);
                  self.current_distance = updated_distance;
                  self.gradient_distance = updated_distance;
                  self.is_first_gradient_frame = false;
               }
               else if is_gradient_update &&
                  let (Some(position), _) = find_closest_point(&self.gpx_track, updated_distance)
               {
                  // println!("Gradient position Update {:?}", position);
                  if position.distance > 0.0
                  {
                     let available_size = ui.available_size();
                     let gradient_offset = self.gradient_offset.load();
                     let offset = (self.gradient_start + gradient_offset).max(self.gradient_end);
                     let gradient_image = match draw_gradient_marker(self, available_size.x, available_size.y, &position)
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
                        self.previous_position = self.current_position;
                        self.current_position = Some(position);
                        // self.current_distance = updated_distance;
                        self.gradient_distance = updated_distance;
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

fn display_streetview(me: &mut GPXAssistUI, ctx: &Context, ui: &mut egui::Ui, requested_delta: f64, updated_distance: f64)
//-----------------------------------------------------------------------------------------------------------------------
{
   if let Some(current_position) = me.current_position
      && let (Some(position), _) = find_closest_point(&me.gpx_track, updated_distance)
   {
      let available_size = ui.available_size();
      let mut errmsg = String::new();
      println!("Streetview: {:.4} {:.4} {:.4}", updated_distance, me.current_distance,  requested_delta);

      let streetview_image = match streetview(ctx, me.encrypted_api_key.as_ref().unwrap(), &current_position,
         available_size.x, available_size.y, true, true)
      {
         | Ok(img) => Some(img),
         | Err(msg) =>
         {
            eprintln!("Error fetching Street View image: {msg}");
            errmsg = msg;
            None

         }
      };
      if let Some(color_image) = streetview_image
      {
         // save_tmp_image(&color_image);
         let texture_name = "streetview_image";
         if me.streetview_texture.is_some()
         {
            me.streetview_texture.as_mut().unwrap().set(color_image, egui::TextureOptions::LINEAR)
         }
         else
         {
            me.streetview_texture = Some(ctx.load_texture(texture_name, color_image, Default::default() ));
         }
      }
      else
      {
         ui.add(egui::Label::new(egui::RichText::new(errmsg).strong().color(egui::Color32::RED) ));
      }

      if let Some(texture) = &me.streetview_texture
      {
         // println!("Texture size: {:?})", texture.size());
         ui.centered_and_justified(|ui|
         {
            // let img = Image::new(&self.streetview_texture);
            // ui.image(texture);
            ui.add(Image::new(texture)
                     .maintain_aspect_ratio(false)
                     .fit_to_exact_size(available_size)
                     .shrink_to_fit()
                  );
         });
      }
      me.previous_position = me.current_position;
      me.current_position = Some(position);
      me.current_distance = updated_distance;
      me.is_first_street_frame = false;
   }
}

fn display_streetview_info(ui: &mut egui::Ui)
//---------------------------------------------
{
   ui.add(egui::Label::new( egui::RichText::new("No Street View API key set in settings.")
            .strong().color(egui::Color32::RED)));
   ui.hyperlink_to( "Click to get a Google Maps API Key (https://console.cloud.google.com/google/maps-apis/)",
      "https://console.cloud.google.com/google/maps-apis/" );
   ui.separator();
   ui.add(egui::Label::new( egui::RichText::new("First 10000 StreetViews per month are free.")
            .color(egui::Color32::GREEN)
   ));
   ui.hyperlink_to( "Click for Pricing Details (https://developers.google.com/maps/billing-and-pricing/pricing#map-loads-pricing)",
         "https://developers.google.com/maps/billing-and-pricing/pricing#map-loads-pricing" );
   let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
   let settings_dir = settings.lock().get_settings_path().unwrap_or(PathBuf::from("."));
   ui.label(format!("After obtaining a key, click the settings button to add API key to settings or modify the settings file {:#?} directly.", settings_dir));
}

// #[allow(clippy::too_many_arguments)]
fn new_gradient_image(me: &mut GPXAssistUI, position: &TrackPoint, width: f32, height: f32, label_width: f64) -> Result<ColorImage, String>
//----------------------------------------------------------------------------------------------------------------------------------
{
   let track = me.gpx_track.clone();
   let total_distance = me.total_distance;
   let gradient_length = me.gradient_length.load();
   let flat_gradient = me.gradient_flat.load();
   let extreme_gradient = me.gradient_extreme.load();
   let gradient_offset = me.gradient_offset.load();
   let extreme_start = extreme_gradient.abs() - 1.5;

   me.gradient_start = (position.distance - gradient_offset).max(0.0);
   me.gradient_end = (me.gradient_start + gradient_length).min(total_distance);
   if me.gradient_end == total_distance
   {
      me.gradient_start = (me.gradient_end - gradient_length).max(0.0);
   }

   //let mut segment_points: Vec<TrackPoint> = Vec::new();
   let mut is_seg_loaded = false;
   let i: i64;
   (_, i) = find_closest_point(&track, me.gradient_start);
   if i >= 0
   {
      let j: i64;
      (_, j) = find_closest_point(&track, me.gradient_end);
      if j >= i
      {
         me.gradient_points = track[i as usize ..= j as usize].to_vec();
         is_seg_loaded = true;
      }
   }
   if ! is_seg_loaded
   {
      me.gradient_points = Vec::new();
      for point in track.iter()
      {
         if point.distance >= me.gradient_start && point.distance <= me.gradient_end
         {
            me.gradient_points.push(*point);
         }
      }
   }

   if me.gradient_points.len() < 2
   {
      return Err("Insufficient points in segment".to_string());
   }

      // Find min/max elevation for scaling
   let min_elevation = me.gradient_points.iter().map(|p| p.altitude).fold(f64::INFINITY, f64::min);
   let max_elevation = me.gradient_points.iter().map(|p| p.altitude).fold(f64::NEG_INFINITY, f64::max);
   let elevation_range = (max_elevation - min_elevation).max(10.0); // Minimum 10m range to avoid division by near-zero

   let pixmap_width = width as u32;
   let pixmap_height = height as u32;
   let mut pixmap = Pixmap::new(pixmap_width, pixmap_height).ok_or_else(|| "Failed to create pixmap".to_string())?;

   pixmap.fill(tiny_skia::Color::from_rgba8(224, 224, 224, 255)); ////BGRA  Skyblue (253, 221, 212, 255) #f0f0f0 to #e0e0e0 or #1e1e1e - #2b2b2b (dark theme) or #222831 - #2a2f3a

   let padding = 60.0;
   let plot_width = width - 2.0 * padding;
   let plot_height = height - 2.0 * padding;
   let distance_range = me.gradient_end - me.gradient_start;

   // Calculate proper aspect ratio with vertical exaggeration
   let vertical_exaggeration = me.vertical_scale.load();
   let actual_aspect_ratio = elevation_range / distance_range; // e.g., 50m / 3000m = 0.0167
   let display_aspect_ratio = actual_aspect_ratio * vertical_exaggeration; // e.g., 0.0167 * 10 = 0.167

   // Calculate the effective plot height based on aspect ratio
   // The elevation should be scaled to fit within the available height while maintaining the aspect ratio
   let effective_plot_height = (plot_width * display_aspect_ratio as f32).min(plot_height);
   let elevation_offset = (plot_height - effective_plot_height) / 2.0; // Center vertically

   let map_to_screen = |dist: f64, elev: f64| -> (f32, f32)
   {
      let x = padding as f64 + ((dist - me.gradient_start) / distance_range) * plot_width as f64;
      let y = padding as f64 + elevation_offset as f64 + effective_plot_height as f64 - ((elev - min_elevation) / elevation_range) * effective_plot_height as f64;
      (x as f32, y as f32)
   };

      // Calculate gradient percentage between two points
      let calculate_gradient_percent = |p1: &TrackPoint, p2: &TrackPoint| -> f64
      {
         let horizontal_dist = p2.distance - p1.distance;
         if horizontal_dist < 0.1 { return 0.0; }
         let vertical_dist = p2.altitude - p1.altitude;
         (vertical_dist / horizontal_dist) * 100.0
      };

      // Get color based on gradient percentage
      let gradient_color = |gradient_pct: f64| -> tiny_skia::Color
      {
         if gradient_pct < -flat_gradient.abs()
         {
            // Downhill: light blue to dark blue
            // let t = ((gradient_pct - flat_gradient.abs()) / extreme_gradient.abs()).min(1.0);
            let t = ((-flat_gradient.abs() - gradient_pct) / extreme_gradient.abs()).abs().min(1.0);
            let b = (255.0) as u8;
            let g = (216.0 * (1.0 - t)) as u8;
            let r = (173.0 * (1.0 - t)) as u8;
            // println!(" (downhill {} {} {})", r, g, b);
            // tiny_skia::Color::from_rgba8(r, g, b, 255)
            tiny_skia::Color::from_rgba8(b, g, r, 255)
         } else if gradient_pct > flat_gradient.abs()
         {
            if gradient_pct >= extreme_gradient.abs()
            {
               tiny_skia::Color::from_rgba8(0, 0, 0, 255)
            }
            else
            {
               // Uphill: light yellow to red
               let t = ((gradient_pct - flat_gradient.abs()) / extreme_gradient.abs()).min(1.0);
               let b = if gradient_pct > extreme_start { 0 } else { 255 };
               let g = ((255.0 * (1.0 - t)) as u8);
               let r = ((150.0 * (1.0 - t)) as u8);
               tiny_skia::Color::from_rgba8(r, g, b, 255)
            }
         }
         else //flat
         {
            // tiny_skia::Color::from_rgba8(50, 200, 50, 255)
            let t = ((flat_gradient.abs() - gradient_pct) / extreme_gradient.abs()).abs().min(1.0);
            let b = 0;
            let g = (255.0 * (1.0 - t)) as u8;
            let r = 0;
            // println!(" (downhill {} {} {})", r, g, b);
            // tiny_skia::Color::from_rgba8(r, g, b, 255)
            tiny_skia::Color::from_rgba8(b, g, r, 255)
         }
      };

      // Draw filled areas and profile line
      for i in 0..me.gradient_points.len() - 1
      {
         let p1 = &me.gradient_points[i];
         let p2 = &me.gradient_points[i + 1];

         let gradient_pct = calculate_gradient_percent(p1, p2);
         let color = gradient_color(gradient_pct);
         // println!("{i}: {}, {} - {}, {} {gradient_pct}", p2.distance, p2.altitude, p1.distance, p1.altitude);
         // {
         //    match OpenOptions::new().append(true).create(true).open("/tmp/gpxdata.txt")
         //    {
         //       | Ok(mut file) =>
         //       {
         //          use std::io::Write;
         //          let log_line = format!("{},{},{},{},{:.2}\n", i, p1.distance, p1.altitude, p2.distance, gradient_pct);
         //          let _ = file.write_all(log_line.as_bytes());
         //       }
         //       | Err(e) =>
         //       {
         //          eprintln!("Error writing to log file: {}", e);
         //       }
         //    }
         // }

         let (x1, y1) = map_to_screen(p1.distance, p1.altitude);
         let (x2, y2) = map_to_screen(p2.distance, p2.altitude);

         // Draw filled polygon below the profile
         let bottom_y = padding + elevation_offset + effective_plot_height;
         let mut path_builder = PathBuilder::new();
         path_builder.move_to(x1, y1);
         path_builder.line_to(x2, y2);
         path_builder.line_to(x2, bottom_y);
         path_builder.line_to(x1, bottom_y);
         path_builder.close();

         if let Some(path) = path_builder.finish()
         {
            let mut paint = Paint::default();
            paint.set_color(color);
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
         }

         // Draw profile line segment
         let mut path_builder = PathBuilder::new();
         path_builder.move_to(x1, y1);
         path_builder.line_to(x2, y2);

         if let Some(path) = path_builder.finish()
         {
            let mut paint = Paint::default();
            paint.set_color(color);
            paint.anti_alias = true;
            let stroke = Stroke { width: 3.0, ..Default::default() };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
         }
      }

   super::frame::draw_distance_labels(&mut pixmap, me.gradient_start, me.gradient_end,
                        label_width, padding, plot_width, plot_height);
   me.gradient_pixmap = Some(Box::new(pixmap.clone()));
   me.gradient_pixmap_width = pixmap_width;
   me.gradient_pixmap_height = pixmap_height;

      let current_position = match me.current_position
      {
         | Some(pos) => pos,
         | None =>
         {
         return Ok(super::frame::pixmap_to_image(&pixmap, pixmap_width, pixmap_height));
         }
      };


   if current_position.distance >= 0.0
   {
      match draw_gradient_marker(me, width, height, &current_position)
      {
         | Ok(img) => return Ok(img),
         | Err(msg) =>
         {
            eprintln!("Error recalculating gradient image: {msg}");
         }
      };
   }
   // Ok((pixmap, pixmap_width, pixmap_height))
   Ok(super::frame::pixmap_to_image(&pixmap, pixmap_width, pixmap_height))
}

fn draw_gradient_marker(me: &mut GPXAssistUI, width: f32, height: f32, position: &TrackPoint) -> Result<ColorImage, String>
//-----------------------------------------
{
   if let Some(gradient_pixmap) = &mut me.gradient_pixmap &&
      me.gradient_points.len() > 0
     //let Some(current_point) = me.gradient_points.iter().find(|p| (p.distance - offset).abs() < 1.0)
   {
      let search_result = me.gradient_points.binary_search_by(|probe|
         probe.distance.partial_cmp(&position.distance).unwrap_or(core::cmp::Ordering::Equal));
      let (mut pt, i) = match search_result
      {
         | Ok(index) => (Some(me.gradient_points[index]), index as i64),
         | Err(index) =>
         {
            let chosen_index = if index == 0 { 0 } else if index >= me.gradient_points.len() { me.gradient_points.len() - 1 }
            else
            {
               let prev = me.gradient_points[index - 1];
               let next = me.gradient_points[index];
               if (position.distance - prev.distance) <= (next.distance - position.distance) { index - 1 } else { index }
            };
            (Some(me.gradient_points[chosen_index]), chosen_index as i64)
         }
      };
      if pt.is_none()
      {
         match me.gradient_points.iter().find(|p| (position.distance - p.distance).abs() < 1.0)
         {
            | Some(p) => pt = Some(*p),
            | None => return Err("Current point not found in gradient points".to_string())
         }
      }
      if let Some(current_point) = pt
      {
         let mut pixmap = (*gradient_pixmap).clone();
         let padding = 60.0;
         let plot_width = width - 2.0 * padding;
         let plot_height = height - 2.0 * padding;
         let distance_range = me.gradient_end - me.gradient_start;
         let min_elevation = me.gradient_points.iter().map(|p| p.altitude).fold(f64::INFINITY, f64::min);
         let max_elevation = me.gradient_points.iter().map(|p| p.altitude).fold(f64::NEG_INFINITY, f64::max);
         let elevation_range = (max_elevation - min_elevation).max(10.0); // Minimum 10m range

         // Calculate proper aspect ratio with vertical exaggeration (same as new_gradient_image)
         let vertical_exaggeration = me.vertical_scale.load();
         let actual_aspect_ratio = elevation_range / distance_range;
         let display_aspect_ratio = actual_aspect_ratio * vertical_exaggeration;
         let effective_plot_height = (plot_width * display_aspect_ratio as f32).min(plot_height);
         let elevation_offset = (plot_height - effective_plot_height) / 2.0;

         let map_to_screen = |dist: f64, elev: f64| -> (f32, f32)
         {
            let x = padding as f64 + ((dist - me.gradient_start) / distance_range) * plot_width as f64;
            let y = padding as f64 + elevation_offset as f64 + effective_plot_height as f64 - ((elev - min_elevation) / elevation_range) * effective_plot_height as f64;
            (x as f32, y as f32)
         };
         let (marker_x, marker_y) = map_to_screen(current_point.distance, current_point.altitude);

         let arrow_size = 15.0;
         let arrow_elevation = 20.0;
         let mut path_builder = PathBuilder::new();
         // path_builder.move_to(marker_x, marker_y - arrow_size); // Top
         // path_builder.line_to(marker_x - arrow_size * 0.6, marker_y + arrow_size * 0.5); // Bottom left
         // path_builder.line_to(marker_x + arrow_size * 0.6, marker_y + arrow_size * 0.5); // Bottom right

         path_builder.move_to(marker_x, marker_y + arrow_size * 0.5 - arrow_elevation); // Top
         path_builder.line_to(marker_x - arrow_size * 0.6, marker_y - arrow_size - arrow_elevation); // Bottom left
         path_builder.line_to(marker_x + arrow_size * 0.6, marker_y - arrow_size - arrow_elevation); // Bottom right
         path_builder.close();

         if let Some(path) = path_builder.finish()
         {
            let mut paint = Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(255, 100, 100, 255));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

            // Draw outline
            let stroke = Stroke { width: 2.0, ..Default::default() };
            paint.set_color(tiny_skia::Color::from_rgba8(0, 0, 0, 255));
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
         }

            // Draw circle at marker position
         let mut path_builder = PathBuilder::new();
         path_builder.push_circle(marker_x, marker_y, 5.0);

         if let Some(path) = path_builder.finish()
         {
            let mut paint = Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(255, 128, 192, 255));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
         }
         Ok(super::frame::pixmap_to_image(&pixmap, me.gradient_pixmap_width, me.gradient_pixmap_height))
      }
      else
      {
         Err("No gradient pixmap or current point available".to_string())
      }
   }
   else
   {
      Err("No gradient pixmap or current point available".to_string())
   }
}

fn gradient_options(me: &mut GPXAssistUI, ui: &mut egui::Ui)
//----------------------------------------------------------
{
   let mut gradient_delta: f64 = me.gradient_delta.load();
   let mut gradient_length: f64 = me.gradient_length.load();
   let mut gradient_position: f64 = me.gradient_offset.load();
   let mut vertical_scale: f64 = me.vertical_scale.load();
   let mut flat_gradient: f64 = me.gradient_flat.load();
   let mut extreme_gradient: f64 = me.gradient_extreme.load();
   ui.horizontal(|ui|
   {
      ui.label(egui::RichText::new("Gradient Refresh:").color(egui::Color32::YELLOW).strong());
      let delta_response = ui.add_sized(
         egui::Vec2::new(100.0, 30.0),
         egui::DragValue::new(&mut gradient_delta)
            .suffix("m")
            .range(1.0..=100.0)
            .speed(10.0))
         .on_hover_text(format!("The distance in metres to travel before redrawing the gradient display with rider positioned at {:.2} (metres)", gradient_position));
      if delta_response.dragged() || delta_response.changed()
      {
         me.gradient_delta.store(gradient_delta);
         // me.is_first_gradient_frame = true;
      }

      ui.add_space(5.0);
      ui.label("Length:");
      let length_response = ui.add_sized(
         egui::Vec2::new(100.0, 30.0),
         egui::DragValue::new(&mut gradient_length)
         .range(100.0..=10000.0)
         .suffix("m")
         .speed(10.0))
         .on_hover_text("The length of the gradient section to display (metres)");
      if length_response.dragged() || length_response.changed()
      {
         me.gradient_length.store(gradient_length);
         me.is_first_gradient_frame = true;
      }

      ui.add_space(5.0);
      ui.label("Offset:");
      let position_response = ui.add_sized(
         egui::Vec2::new(100.0, 30.0),
         egui::DragValue::new(&mut gradient_position)
            .suffix("m")
            .range(100.0..=2000.0)
            .speed(10.0))
         .on_hover_text("The position within the gradient section where the rider currently is positioned (metres)");
      if position_response.dragged() || position_response.changed()
      {
         me.gradient_offset.store(gradient_position);
         me.is_first_gradient_frame = true;
      }
   });
   ui.horizontal(|ui|
   {
      ui.label("Vertical Scale:");
      let scaling_response = ui.add_sized(
         egui::Vec2::new(100.0, 30.0),
         egui::DragValue::new(&mut vertical_scale)
         .range(1.0..=50.0)
         .speed(0.5)
         .max_decimals(1))
         .on_hover_text("Vertical scaling for gradient");
      if scaling_response.dragged() || scaling_response.changed()
      {
         me.vertical_scale.store(vertical_scale);
         me.is_first_gradient_frame = true;
      }

      ui.separator();
      ui.label("Flat Gradient (%):");
      let flat_gradient_response = ui.add_sized(
         egui::Vec2::new(100.0, 30.0),
         egui::DragValue::new(&mut flat_gradient)
         .suffix("%")
         .range(0.1..=2.5)
         .speed(0.1)
         .max_decimals(1))
         .on_hover_text("The gradient considered to be 'flat', e.g if 0.5 then -0.5 to 0.5 is flat");
      if flat_gradient_response.dragged() || flat_gradient_response.changed()
      {
         me.gradient_flat.store(flat_gradient);
         me.is_first_gradient_frame = true;
      }

      ui.label("Extreme Gradient (%):");
      let extreme_gradient_response =  ui.add_sized(
         egui::Vec2::new(100.0, 30.0),
         egui::DragValue::new(&mut extreme_gradient)
         .range(10.0..=100.0)
         .speed(0.5)
         .max_decimals(1))
         .on_hover_text("The gradient considered to be 'extreme' (black), e.g if > 16 then gradient color is black");
      if extreme_gradient_response.dragged() || extreme_gradient_response.changed()
      {
         me.gradient_extreme.store(extreme_gradient);
         me.is_first_gradient_frame = true;
      }
   });
}

fn render_current_gradient(me: &mut GPXAssistUI, ui: &mut egui::Ui)
//------------------------------------------------------
{
   ui.vertical(|ui|
   {
      gradient_options(me, ui);
      // ui.centered_and_justified(|ui|
      // {
      if let Some(texture) = &me.gradient_texture
      {
         ui.add(Image::new(texture)
                  .maintain_aspect_ratio(true)
                  .fit_to_original_size(1.0)
                  .shrink_to_fit()
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

   let img = image::load_from_memory(png_data)
      .map_err(|e| format!("Failed to decode PNG: {}", e))?;

   let rgba = img.to_rgba8();
   let size = [rgba.width() as usize, rgba.height() as usize];
   let pixels = rgba.into_raw();

   Ok(ColorImage::from_rgba_unmultiplied(size, &pixels))
}

fn display_invalid_broadcast_directory(ui: &mut egui::Ui, is_aged: bool, delta: f64)
//----------------------------------------------------
{
   let broadcast_file = match get_broadcast_file()
   {
      | Some(dir) => dir,
      | None => PathBuf::from(""),
   };
   let err_color:  Color32;
   let errmsg = if broadcast_file.is_file() && is_aged
   {
      err_color = Color32::YELLOW;
      format!("The broadcast file {:?} has not been updated recently enough (try pedalling for more than {} metres).", broadcast_file, delta).to_string()
   }
   else
   {
      err_color = Color32::RED;
      format!("Could not find a valid TrainingPeaks Virtual broadcast file at {:#?}.", broadcast_file).to_string()
   };

   // Load embedded PNG images - unwrap is safe since assets are embedded at compile time
   let color_img_1 = load_embedded_png("menu-1.png").expect("menu-1.png should be embedded");
   let texture_1 = ui.ctx().load_texture("menu_1", color_img_1, Default::default());
   let image_1 = Image::new(&texture_1)
      .maintain_aspect_ratio(true)
      .fit_to_fraction(Vec2 { x: 0.1, y: 0.5 })
      .shrink_to_fit();

   let color_img_2 = load_embedded_png("menu-2.png").expect("menu-2.png should be embedded");
   let texture_2 = ui.ctx().load_texture("menu_2", color_img_2, Default::default());
   let image_2 = Image::new(&texture_2)
      .max_size(Vec2 { x: 115.0, y: 142.0 })
      .shrink_to_fit();

   let color_img_3 = load_embedded_png("menu-3.png").expect("menu-3.png should be embedded");
   let texture_3 = ui.ctx().load_texture("menu_3", color_img_3, Default::default());
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
      let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
      let settings_dir = settings.lock().get_settings_path().unwrap_or(PathBuf::from("."));
      let errmsg = format!(r#"If the broadcast file location is still incorrect, use the path below the "Save to Local File" as shown in the image above either in the settings dialog or modify the settings file {:#?} directly."#, settings_dir);
      ui.add(egui::Label::new(
         egui::RichText::new(errmsg)
                  .color(egui::Color32::LIGHT_YELLOW)));
   });
}


fn open_file_dialog(ctx: &Context, sender: Sender<(Vec<TrackPoint>, String)>)
//--------------------------------------------------------------------------
{
   let pick_dir: PathBuf;
   {
      let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
      pick_dir = settings.lock().get_last_directorybuf();
   }
   let dialog_future = rfd::AsyncFileDialog::new().set_directory(pick_dir).pick_file();
   let ctxx = ctx.clone();
   execute(async move
   {
      let file_info = dialog_future.await;
      if let Some(fileinfo) = file_info
      {
         let path = fileinfo.path();
         match path.parent()
         {
            | Some(d) =>
            {
               let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
               settings.lock().set_last_directorybuf(&d.to_path_buf());
            },
            | None => (),
         };
         let file_path_disp = &path.display();
         let track_data: Vec<TrackPoint> = match process_gpx(file_path_disp.to_string().clone().as_str())
         {
            | Ok(trackdata) =>
            {
               println!("Successfully processed {} points.", trackdata.len());
               trackdata
            }
            | Err(e) =>
            {
               eprintln!("Error processing GPX file {:?}: {}", fileinfo.path(), e);
               Vec::new()
            }
         };
         let _ = sender.send((track_data, file_path_disp.to_string().clone()));
         // let _ = sender.send(String::from_utf8_lossy(&text).to_string());
         ctxx.request_repaint();
      }
   });
}

fn execute<F: Future<Output = ()> + Send + 'static>(f: F)
{
    std::thread::spawn(move || futures::executor::block_on(f));
}

fn set_style(ctx: &Context)
//--------------------
{
   let mut style: egui::Style = (*ctx.style()).clone();
   style.visuals.window_fill = egui::Color32::from_rgb(30, 30, 30);
   style.visuals.image_loading_spinners = true;
   style.text_styles = [(egui::TextStyle::Heading, egui::FontId::new(30.0, egui::FontFamily::Proportional)),
                        (egui::TextStyle::Body, egui::FontId::new(20.0, egui::FontFamily::Proportional)),
                        (egui::TextStyle::Monospace, egui::FontId::new(20.0, egui::FontFamily::Monospace)),
                        (egui::TextStyle::Button, egui::FontId::new(20.0, egui::FontFamily::Proportional)),
                        (egui::TextStyle::Small, egui::FontId::new(15.0, egui::FontFamily::Proportional))].into();
   ctx.set_style(style);
}

pub fn streetview( ctx: &Context, api_key: &str, position: &TrackPoint, width: f32, height: f32,
   use_heading: bool, is_debug: bool ) -> Result<ColorImage, String>
//--------------------------
{
   // Default parameters for Street View
   let fov = 90;      // Field of view (0-120 degrees)
   let heading = position.heading as i32; // Heading in degrees (0-360)
   let current_latitude = position.point.lat;
   let current_longitude = position.point.lon;
   let pitch = 0;     // Up/down angle (-90 to 90 degrees)
   let w = width as u32; // width.min(640.0).round() as u32;
   let h = height as u32; // height.min(640.0).round() as u32;

   // Construct the Google Street View API URL
   let url: String;
   if use_heading
   {
      url = format!(
         "https://maps.googleapis.com/maps/api/streetview?size={w}x{h}&location={current_latitude},{current_longitude}&fov={fov}&heading={heading}&pitch={pitch}&key={api_key}");
   }
   else
   {
      url = format!(
         "https://maps.googleapis.com/maps/api/streetview?size={w}x{h}&location={current_latitude},{current_longitude}&fov={fov}&pitch={pitch}&key={api_key}");
   }
   println!("Fetching Street View from: {}", url);

   // Fetch and load the image
   fetch_image_from_url(&url)
}

/// Helper function to draw distance labels on the gradient profile
pub(crate) fn draw_distance_labels(pixmap: &mut tiny_skia::Pixmap, segment_start_distance: f64, segment_end_distance: f64,
                        label_width: f64, padding: f32, plot_width: f32, plot_height: f32)
//---------------------------------------------------------------------------------------------------------------
{
    use fontdue::{Font, FontSettings};

    // Embedded font data (using a simple fallback)
    const FONT_DATA: &[u8] = include_bytes!("../../assets/Roboto-Regular.ttf");

    let font = match Font::from_bytes(FONT_DATA, FontSettings::default()) {
        Ok(f) => f,
        Err(_) => return, // Skip labels if font fails to load
    };

    let font_size = 14.0;
    let label_y = padding + plot_height + 25.0;
    let distance_range = segment_end_distance - segment_start_distance;

    // Calculate number of labels based on label_width
    let num_labels = (distance_range / label_width).ceil() as usize + 1;

    for i in 0..num_labels
    {
        let distance_at_label = segment_start_distance + (i as f64 * label_width);
        if distance_at_label > segment_end_distance
        {
            break;
        }

        // Convert distance to km for display
        let distance_km = distance_at_label / 1000.0;
        let label_text = format!("{:.1}km", distance_km);

        // Calculate x position for this label
        let x = padding as f64 + ((distance_at_label - segment_start_distance) / distance_range) * plot_width as f64;

        // Render the text
        let mut x_offset = x as f32;
        let pixmap_width = pixmap.width();
        let pixmap_height = pixmap.height();

        for ch in label_text.chars() {
            let (metrics, bitmap) = font.rasterize(ch, font_size);

            // Draw each pixel of the character
            for (py, row) in bitmap.chunks(metrics.width).enumerate() {
                for (px, &alpha) in row.iter().enumerate() {
                    if alpha > 0 {
                        let pixel_x = (x_offset + px as f32) as u32;
                        let pixel_y = (label_y + py as f32) as u32;

                        if pixel_x < pixmap_width && pixel_y < pixmap_height {
                            let color = tiny_skia::Color::from_rgba8(0, 0, 0, alpha);
                            pixmap.pixels_mut()[((pixel_y * pixmap_width + pixel_x) as usize)] =
                                color.premultiply().to_color_u8();
                        }
                    }
                }
            }
            x_offset += metrics.advance_width;
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
            let stroke = tiny_skia::Stroke { width: 2.0, ..Default::default() };
            pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
        }
    }
}

pub(crate) fn pixmap_to_image(pixmap: &tiny_skia::Pixmap, pixmap_width: u32, pixmap_height: u32) -> ColorImage
//-----------------------------------------------
{
   let pixels = pixmap.data();
   let mut rgba_pixels = Vec::with_capacity((pixmap_width * pixmap_height * 4) as usize);

   for chunk in pixels.chunks_exact(4)
   {
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
   let response = reqwest::blocking::get(url)
      .map_err(|e| format!("Failed to fetch image: {}", e))?;

   // Check response status
   let status = response.status();
   if !status.is_success() {
      return Err(format!("HTTP error: {} - Check if location has Street View coverage", status));
   }

   let bytes = response.bytes()
      .map_err(|e| format!("Failed to read response: {}", e))?;

   // Check if we got actual image data
   if bytes.len() < 100 {
      return Err("Received suspiciously small response - location may not have Street View coverage".to_string());
   }

   // Decode the image
   let img = image::load_from_memory(&bytes)
      .map_err(|e| format!("Failed to decode image: {}", e))?;

   let rgba = img.to_rgba8();
   let size = [rgba.width() as usize, rgba.height() as usize];
   let pixels = rgba.into_raw();

   println!("Decoded image: {}x{}, {} bytes", size[0], size[1], pixels.len());

   Ok(ColorImage::from_rgba_unmultiplied(size, &pixels))
}

pub fn get_broadcast_directory() -> Option<PathBuf>
//---------------------------------------------
{
   if cfg!(target_os = "macos")
   {  // ~/TPVirtual/Broadcast/focus.json
      match dirs::home_dir()
      {
         | Some(dir) =>
         {
            Some(dir.join("TPVirtual").join("Broadcast").clone())
         },
         | None => None,
      }
   }
   else
   {
      match dirs::document_dir()
      {
         | Some(dir) =>
         {
            Some(dir.join("TPVirtual").join("Broadcast").clone())
         },
         | None => None,
      }
   }
}

pub fn get_broadcast_file() -> Option<PathBuf>
//---------------------------------------------
{
   match get_broadcast_directory()
   {
      | Some(dir) =>
      {
         Some(dir.join("focus.json")).clone()
      },
      | None => None,
   }
}

/// Returns the distance in meters from the broadcast focus.json file.
/// -1 indicates an error parsing the file after parse_retries attempts.
pub(crate) fn read_rider_data(parse_retries: i64, retry_duration: Duration) -> Option<RiderDataJSON>
//--------------------------------------
{
   let broadcast_file = match get_broadcast_file()
   {
      | Some(f) =>
      {
         if ! f.exists()
         {
            return None;
         }
         else
         {
            f
         }
      },
      | None => { return None; }
   };

   for _ in 0..parse_retries
   {
      let rider_json_data = match std::fs::read_to_string(&broadcast_file)
      {
         | Ok(data) =>
         {
            //.ok()?.trim().to_string(); //[{"name":"xxx"....}]
            let s = data.trim().to_string();
            if s.is_empty()
            {
               return None;
            }
            s
         }
         | Err(_) => { return None; }
      };

      // The data as read from disk has 3 binary characters at the start which cause JSON parsing to fail.
      // Turns out its a UTF-8 BOM (Byte Order Mark) (https://en.wikipedia.org/wiki/Byte_order_mark)
      // which Rusts standard library does not strip automatically.
      let mut pch = rider_json_data.find('[');
      if pch.is_none()
      {
         pch = rider_json_data.find('{');
         if pch.is_none() { return None; }
      }

      let p = pch.unwrap_or(0);
      let rider_json_data = if p > 0
      {
         rider_json_data[p..].to_string()
      }
      else
      {
         rider_json_data
      };

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
      let rider_json = rider_json_data.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
         .unwrap_or(&rider_json_data).to_string().trim().to_string();

      // println!("Process rider JSON: {}", rider_json);

      if let Ok(rider_data) = RiderDataJSON::from_json(&rider_json)
      {
         return Some(rider_data);
      }
      std::thread::sleep(retry_duration);
   }
   None
}
