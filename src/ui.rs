use std::{collections::HashMap, path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{Receiver, Sender, channel}}, time::Duration};

use tempfile::NamedTempFile;
use crossbeam::atomic::AtomicCell;

use chrono::{Local, DateTime};
use eframe::{CreationContext, egui::{self, ColorImage, Context, Frame, Image, TextureHandle, Vec2}, emath::Numeric};
use walkers::{HttpTiles, Map, MapMemory, lon_lat, sources::OpenStreetMap};
use include_dir::{include_dir, Dir};

use crate::{STARTUP_PARAMS, components::{DirectionalArrow, ToastManager}, data::RiderDataJSON, data::RiderData, gpx::{DistanceMethod, TrackPoint, find_closest_point, process_gpx}};
use crate::SETTINGS;
use crate::settings::Settings;
use crate::ut;

// Embed the entire assets directory at compile time
static ASSETS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets");

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ViewMode
{
   NA,
   Map,
   StreetView,
   Gradient
}

const MENU_HEIGHT: u32 = 48;

pub struct GPXAssistUI
//====================
{
   toast_manager:          ToastManager,
   encrypted_api_key:      Option<String>, //TODO: Actually do encrypted version
   distance_method:        Option<DistanceMethod>,
   is_first_map_frame:     bool,
   is_first_street_frame:  bool,
   is_first_gradient_frame:bool,
   gpx_file:               Option<PathBuf>,
   gpx_track:              Arc<Vec<TrackPoint>>,
   total_distance:         f64,
   current_distance:       f64,
   updated_distance:       Arc<AtomicCell<f64>>,
   requested_delta:        Arc<AtomicCell<f64>>,
   simulated_speed:        Arc<AtomicCell<f64>>,
   current_mode:           ViewMode,
   textures:               HashMap<String, (TextureHandle, [f32; 2])>,
   previous_position:      Option<TrackPoint>,
   current_position:       Option<TrackPoint>,
   open_dialog_channel:    (Sender<(Vec<TrackPoint>, String)>, Receiver<(Vec<TrackPoint>, String)>),
   tiles:                  Option<HttpTiles>,
   map_memory:             Option<MapMemory>,
   streetview_texture:     Option<TextureHandle>,
   gradient_texture:       Option<TextureHandle>,
   is_simulating:          Arc<AtomicBool>,
   is_running:             Arc<AtomicBool>,
   rider_data:             Arc<AtomicCell<RiderData>>,
   show_settings_dialog:   bool,
   show_api_key:           bool,
   temp_api_key:           String,
   temp_gradient_length:   f64,
   temp_gradient_position: f64,
   temp_flat_gradient:     f64,
   temp_extreme_gradient:  f64,
}

impl Default for GPXAssistUI
//===========================
{
   fn default() -> Self
//------------------
   {
      let cmdline_opts = STARTUP_PARAMS.lock();
      let cmdline_opts = cmdline_opts.borrow();
      let method_opt = cmdline_opts.as_ref().and_then(|opts| opts.method);
      let filepath_opt = cmdline_opts.as_ref()
                                     .and_then(|opts| opts.file_path.as_ref().map(|s| PathBuf::from(s)));
      let track_data_opt: Option<Vec<TrackPoint>>;
      let mut total_distance: f64 = 0.0;
      let tiles_opt: Option<HttpTiles> = None;
      let map_memory_opt: Option<MapMemory> = None;
      let mut previous_position = None;
      let mut current_position = None;
      if Some(filepath_opt.is_some()).unwrap_or(false)
      {
         let file_path = filepath_opt.as_ref().unwrap().to_str().unwrap();
         let method = method_opt.unwrap_or(DistanceMethod::Haversine);
         let track_data: Vec<TrackPoint> = match process_gpx(&file_path, method)
         {
            | Ok(track_data) =>
            {
               println!("Successfully processed {} points.", track_data.len());
               total_distance = track_data.last().map_or(0.0, |p| p.distance);
               current_position = track_data.first().map(|p| *p);
               previous_position = current_position;
               track_data
            }
            | Err(e) =>
            {
               eprintln!("Error processing GPX file {file_path}: {e}");
               Vec::new()
            }
         };
         track_data_opt = Some(track_data);
      }
      else
      {
         track_data_opt = None;
      }
      let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
      let mut api_key =  settings.lock().get_streetview_api_key().ok();
      if api_key.is_some() && api_key.as_ref().unwrap().is_empty()
      {
         api_key = None
      }
      Self
      {
         toast_manager: ToastManager::new(),
         encrypted_api_key: api_key,
         distance_method: method_opt,
         is_first_map_frame : true,
         // first_map_count : 3,
         is_first_street_frame : true,
         is_first_gradient_frame : true,
         gpx_file: filepath_opt,
         gpx_track: Arc::new(track_data_opt.unwrap_or_default()),
         total_distance,
         current_distance: 0.0,
         updated_distance: Arc::new(AtomicCell::new(0.0)),
         requested_delta: Arc::new(AtomicCell::new(100.0)),
         simulated_speed: Arc::new(AtomicCell::new(45.0)),
         textures: HashMap::new(),
         current_mode: ViewMode::NA,
         previous_position,
         current_position,
         open_dialog_channel: channel(),
         tiles: tiles_opt,
         map_memory: map_memory_opt,
         streetview_texture: None,
         gradient_texture: None,
         is_simulating: Arc::new(AtomicBool::new(false)),
         is_running: Arc::new(AtomicBool::new(false)),
         rider_data: Arc::new(AtomicCell::new(RiderData::default())),
         show_settings_dialog: false,
         show_api_key: false,
         temp_api_key: String::new(),
         temp_gradient_length: 3000.0,
         temp_gradient_position: 500.0,
         temp_flat_gradient: 0.5,
         temp_extreme_gradient: 16.0,
      }
   }
}

impl GPXAssistUI
//==============
{
   pub fn new(cc: &CreationContext) -> Self
//----------------------
   {
      let mut app = GPXAssistUI::default();
      match load_svg_texture(&cc.egui_ctx, "open_icon", "open_icon.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) =>
         {
            app.textures
               .insert("open".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) =>
         {
            eprintln!("Failed to load open icon texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "test_on_icon", "test_icon.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) =>
         {
            app.textures
               .insert("test-on".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) =>
         {
            eprintln!("Failed to load test icon texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "test_off_icon", "test_off_icon.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) =>
         {
            app.textures
               .insert("test-off".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) =>
         {
            eprintln!("Failed to load test off icon texture {e}.");
         }
      }

      match load_svg_texture(&cc.egui_ctx, "map_on_icon", "globe-on.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) =>
         {
            app.textures
               .insert("map-on".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) =>
         {
            eprintln!("Failed to load map on texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "map_off_icon", "globe-off.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) =>
         {
            app.textures
               .insert("map-off".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) =>
         {
            eprintln!("Failed to load map off texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "street_on_icon", "streetview-on.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) =>
         {
            app.textures
               .insert("street-on".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) =>
         {
            eprintln!("Failed to load streetview on icon texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "street_off_icon", "streetview-off.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) =>
         {
            app.textures
               .insert("street-off".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) =>
         {
            eprintln!("Failed to load streetview off icon texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "settings_icon", "settings.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) =>
         {
            app.textures
               .insert("settings".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) =>
         {
            eprintln!("Failed to load settings icon texture {e}.");
         }
      }
      app.tiles = Some(HttpTiles::new(OpenStreetMap, cc.egui_ctx.clone()));
      app.map_memory = Some(MapMemory::default());

      // // Initialize streetview_texture with a 1x1 transparent placeholder
      // let placeholder = ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 0]);
      // app.streetview_texture = cc.egui_ctx.load_texture(
      //    "streetview_placeholder",
      //    placeholder,
      //    egui::TextureOptions::LINEAR
      // );

      app
   }

   /// Opens the settings dialog and loads current settings into temp fields
   fn open_settings_dialog(&mut self)
   //---------------------------------
   {
      // Load current settings into temp fields
      let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
      let settings_lock = settings.lock();

      // Try to get the decrypted API key
      if let Ok(api_key) = settings_lock.get_streetview_api_key()
      {
         self.temp_api_key = api_key;
      }
      else
      {
         self.temp_api_key.clear();
      }

      self.temp_gradient_length = settings_lock.gradient_length;
      self.temp_gradient_position = settings_lock.gradient_position;
      self.temp_flat_gradient = settings_lock.flat_gradient_percentage;
      self.temp_extreme_gradient = settings_lock.extreme_gradient_percentage;

      // Reset API key visibility
      self.show_api_key = false;

      // Show the dialog
      self.show_settings_dialog = true;
   }

   /// Shows the settings dialog if it's open (call this every frame)
   fn show_settings_dialog(&mut self, ctx: &Context)
   //------------------------------------------------
   {
      if !self.show_settings_dialog
      {
         return;
      }

      egui::Window::new("Settings")
         .collapsible(false)
         .resizable(false)
         .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
         .show(ctx, |ui| {
            ui.set_min_width(500.0);

            egui::Grid::new("settings_grid")
               .num_columns(2)
               .spacing([10.0, 10.0])
               .striped(true)
               .show(ui, |ui| {
                  // Google Street View API Key
                  ui.label("Street View API Key:");
                  ui.horizontal(|ui| {
                     ui.add(egui::TextEdit::singleline(&mut self.temp_api_key)
                        .hint_text("Enter your Google API key")
                        .password(!self.show_api_key)
                        .desired_width(300.0))
                        .on_hover_text("Enter your Google API key");

                     // Toggle button to show/hide API key
                     let button_text = if self.show_api_key { "🙈 Hide" } else { "👁 Show" };
                     if ui.button(button_text).clicked() {
                        self.show_api_key = !self.show_api_key;
                     }
                  });
                  ui.end_row();

                  ui.label("Gradient Length (m):");
                  ui.add_sized(
                     egui::Vec2::new(100.0, 30.0),
                     egui::DragValue::new(&mut self.temp_gradient_length)
                     .range(500.0..=10000.0)
                     .speed(10.0))
                     .on_hover_text("The length of the gradient section to display (metres)");
                  ui.end_row();

                  ui.label("Gradient Offset (m):");
                  ui.add_sized(
                     egui::Vec2::new(100.0, 30.0),
                     egui::DragValue::new(&mut self.temp_gradient_position)
                     .range(100.0..=2000.0)
                     .speed(10.0))
                     .on_hover_text("The position within the gradient section where the rider currently is positioned (metres)");
                  ui.end_row();

                  ui.label("Flat Gradient (%):");
                  ui.add_sized(
                     egui::Vec2::new(100.0, 30.0),
                     egui::DragValue::new(&mut self.temp_flat_gradient)
                     .range(0.1..=2.0)
                     .speed(0.1)
                     .max_decimals(1))
                     .on_hover_text("The gradient considered to be 'flat', e.g if 0.5 then -0.5 to 0.5 is flat");
                  ui.end_row();

                  ui.label("Extreme Gradient (%):");
                  ui.add_sized(
                     egui::Vec2::new(100.0, 30.0),
                     egui::DragValue::new(&mut self.temp_extreme_gradient)
                     .range(10.0..=25.0)
                     .speed(0.5)
                     .max_decimals(1))
                     .on_hover_text("The gradient considered to be 'extreme' (black), e.g if > 16 then gradient color is black");
                  ui.end_row();
               });

            ui.separator();

            ui.horizontal(|ui| {
               if ui.button("Save").clicked()
               {
                  // Save settings
                  let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
                  let mut settings_lock = settings.lock();

                  // Save API key
                  if !self.temp_api_key.is_empty()
                  {
                     match settings_lock.set_streetview_api_key(&self.temp_api_key)
                     {
                        | Ok(_) =>
                        {
                           self.toast_manager.success("Settings saved successfully", Some(Duration::from_secs(3)));
                           self.encrypted_api_key = Some(self.temp_api_key.clone());
                        }
                        | Err(e) =>
                        {
                           self.toast_manager.error(&format!("Failed to save API key: {}", e), None);
                        }
                     }
                  }

                  // Update gradient settings
                  settings_lock.gradient_length = self.temp_gradient_length;
                  settings_lock.gradient_position = self.temp_gradient_position;
                  settings_lock.flat_gradient_percentage = self.temp_flat_gradient;
                  settings_lock.extreme_gradient_percentage = self.temp_extreme_gradient;

                  // Write settings to file
                  match settings_lock.write_settings()
                  {
                     | Ok(_) => (),
                     | Err(e) =>
                     {
                        self.toast_manager.error(&format!("Failed to write settings: {}", e), None);
                     }
                  }

                  // Close dialog
                  self.show_settings_dialog = false;
               }

               if ui.button("Cancel").clicked()
               {
                  // Reset temp values
                  self.temp_api_key.clear();
                  self.temp_gradient_length = 3000.0;
                  self.temp_gradient_position = 500.0;
                  self.temp_flat_gradient = 0.5;
                  self.temp_extreme_gradient = 16.0;
                  self.show_api_key = false;

                  // Close dialog
                  self.show_settings_dialog = false;
               }
            });
         });
   }

   #[allow(clippy::too_many_arguments)]
   fn update_distance_thread(ctx: Context, updated_distance: Arc<AtomicCell<f64>>,  track: Arc<Vec<TrackPoint>>,
     requested_delta: Arc<AtomicCell<f64>>, rider_data: Arc<AtomicCell<RiderData>>, total_distance: f64,
     is_running: Arc<AtomicBool> )
   //--------------------------------------------------------------------------------------------------------------------
   {
      let mut last_distance: f64 = 0.0;
      let mut distance: f64 = 0.0;
      while distance < total_distance
      {
         if !is_running.load(Ordering::Relaxed)
         {
            std::thread::sleep(Duration::from_secs(1));
            continue;
         }
         let mut rider = match read_rider_data(3, Duration::from_millis(300))
         {
            | Some(r) => r,
            | None =>
            {
               std::thread::sleep(Duration::from_secs(1));
               continue;
            }
         };

         distance = rider.distance_meters();
         // println!("Read distance: {:.2} meters ({:.2}km)", distance, distance / 1000.0);
         if distance > last_distance
         {
            let distance_delta = requested_delta.load();
            if (distance - last_distance) >= distance_delta
            {
               updated_distance.store(distance);
               last_distance = distance;
               if let (Some(position), _) = find_closest_point(&track, distance)
               {
                  rider.latitude = position.point.lat;
                  rider.longitude = position.point.lon;
                  rider.altitude = position.altitude;
               }
               let rider_copy = RiderData::from(rider);
               rider_data.store(rider_copy);
               ctx.request_repaint();
               println!("Sent distance: {:.2} meters ({:.2}km)", distance, distance / 1000.0);
            }
         }

         // if !is_running.load(Ordering::Relaxed) { break; }
         std::thread::sleep(Duration::from_secs(1));
      }
   }

   /// Simulates movement along a GPX track at 45km/h
   #[allow(clippy::too_many_arguments)]
   fn simulate_movement_thread( ctx: Context, updated_distance: Arc<AtomicCell<f64>>, track: Arc<Vec<TrackPoint>>,
      requested_delta: Arc<AtomicCell<f64>>, simulated_speed: Arc<AtomicCell<f64>>, rider_data: Arc<AtomicCell<RiderData>>,
      total_distance: f64, is_sim_running: Arc<AtomicBool>, is_running: Arc<AtomicBool> )
   //-------------------------------------------------------------------------------------------------
   {
      let mut distance: f64 = 0.0;
      let mut distance_delta = requested_delta.load();
      let mut last_distance: f64 = -distance_delta;
      let speed = simulated_speed.load();
      let speed: f64 = 45.0 * 1000.0 / (60.0 * 60.0); // km/h to m/s
      let start: DateTime<Local> = Local::now();
      while distance < total_distance
      {
         if is_running.load(Ordering::Relaxed)
         {
            break;
         }
         if (distance - last_distance) >= distance_delta
         {
            updated_distance.store(distance);
            let mut rider = RiderData { distance: distance as i32, ..Default::default() }; //::default();
            // rider.distance = distance as i32;
            if let (Some(position), _) = find_closest_point(&track, distance)
            {
               rider.latitude = position.point.lat;
               rider.longitude = position.point.lon;
               rider.altitude = position.altitude;
            }
            rider.wind_speed = 10;
            rider.wind_angle = 60;
            rider_data.store(rider);
            last_distance = distance;
            ctx.request_repaint();
            println!("Simulated distance: {:.2} meters ({:.2}km)", distance, distance / 1000.0);
         }
         let now: DateTime<Local> = Local::now();
         let total_time = (now - start).num_seconds() as f64;
         distance = speed * total_time;
         updated_distance.store(distance);

         if !is_sim_running.load(Ordering::Relaxed)
         {
            break;
         }
         std::thread::sleep(Duration::from_secs(1));
         distance_delta = requested_delta.load();
      }
      is_sim_running.store(false, Ordering::Relaxed);
      is_running.store(true, Ordering::Relaxed);
   }

   fn check_update_file(&mut self)
   //----------------------------------
   {
      let broadcast_file = get_broadcast_file();
      let is_exists = broadcast_file.is_some() && broadcast_file.as_ref().unwrap().is_file();
      let mut age: chrono::Duration = chrono::Duration::zero();
      if is_exists
      {
         age = match ut::get_file_age(broadcast_file.as_ref().unwrap())
         {
            | Ok(d) => d,
            | Err(e) =>
            {
               eprintln!("Error getting broadcast file age: {}", e);
               return;
            }
         };
      }
      let is_aged = age.num_minutes() > 1;
      if ! is_exists || is_aged
      {
         let age_msg = if is_aged
         {
            " or the broadcast file has not been updated recently "
         }
         else
         {
            ""
         };
         let errmsg = format!("Could not find a valid TrainingPeaks Virtual broadcast file{}at {:#?}", age_msg, broadcast_file);
         self.toast_manager.warning(errmsg, Some(Duration::from_secs(10)));
         return;
      }
   }
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
                  self.current_mode = ViewMode::Map;
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
                  let updated_distance = self.updated_distance.clone();
                  let requested_delta = self.requested_delta.clone();
                  let rider_data = self.rider_data.clone();
                  let total_distance = self.total_distance;
                  let is_running = self.is_running.clone();
                  let track = self.gpx_track.clone();
                  let ctxx = ctx.clone();
                  self.is_first_map_frame = false;
                  self.is_first_street_frame = false;
                  self.is_first_gradient_frame = false;
                  self.check_update_file();
                  std::thread::spawn(move ||
                  {
                     Self::update_distance_thread(ctxx, updated_distance, track, requested_delta, rider_data, total_distance, is_running);
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
                  self.open_settings_dialog();
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
                  ui.label(egui::RichText::new("Delta:").color(egui::Color32::YELLOW).strong());
                  let distance_response = ui.add_sized(
                     egui::Vec2::new(80.0, 30.0), // Fixed size: width = 80, height = 30
                     egui::DragValue::new(&mut dist)
                        .range(0.0..=10000.0)
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

                  let before = self.current_mode.clone();
                  ui.selectable_value(&mut self.current_mode, ViewMode::Map,
                     egui::RichText::new("Map").color(egui::Color32::LIGHT_YELLOW));
                  ui.selectable_value(&mut self.current_mode, ViewMode::StreetView,
                     egui::RichText::new("StreetView").color(egui::Color32::LIGHT_YELLOW));
                  ui.selectable_value(&mut self.current_mode, ViewMode::Gradient,
                     egui::RichText::new("Gradient").color(egui::Color32::LIGHT_YELLOW));
                  if before != self.current_mode
                  {
                     if before == ViewMode::Map
                     {
                        self.is_first_map_frame = false;
                     }
                     if before == ViewMode::StreetView
                     {
                        self.is_first_street_frame = false;
                     }
                     if before == ViewMode::Gradient
                     {
                        self.is_first_gradient_frame = false;
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
                     let simulated_speed = self.simulated_speed.clone();
                     let total_distance = self.total_distance;
                     let is_running = self.is_running.clone();
                     let is_sim_running = self.is_simulating.clone();
                     let track = self.gpx_track.clone();
                     let ctxx = ctx.clone();
                     std::thread::spawn(move ||
                     {
                        Self::simulate_movement_thread(ctxx, updated_distance, track, requested_delta, simulated_speed, rider_data, total_distance, is_sim_running, is_running);
                     });
                  }
               }
            })
         } );

         egui::CentralPanel::default()
         .show(ctx, |ui|
         {
            let broadcast_file= get_broadcast_file();
            if self.current_mode == ViewMode::NA || self.gpx_file.is_none() || self.total_distance == 0.0
            {
               let available_size = ui.available_size();
               let image = Image::new(egui::include_image!("../assets/GPXAssist.png"))
                  .maintain_aspect_ratio(false)
                  .fit_to_exact_size(available_size)
                  .shrink_to_fit();

               ui.centered_and_justified(|ui|
               {
                  ui.add(image);
               });
            }
            else if ! self.is_simulating.load(Ordering::Relaxed) && ( (broadcast_file.is_none() || !broadcast_file.as_ref().unwrap().is_file()) )
            {
               let age = match ut::get_file_age(broadcast_file.as_ref().unwrap())
               {
                  | Ok(d) => d.num_minutes(),
                  | Err(_e) =>
                  {
                     // eprintln!("Error getting broadcast file age: {}", e);
                     -1 as i64
                  }
               };
               let is_aged = age >= 1;
               display_invalid_broadcast_directory(ui, is_aged);
            }
            else
            {
               let updated_distance = self.updated_distance.load();
               let rider_data = self.rider_data.load();
               // let updated_distance = rider_data_clone.distance_meters();
               let requested_delta = self.requested_delta.load();
               let is_update = (requested_delta > 20.0) && (updated_distance - self.current_distance) >= requested_delta;  //
               if self.current_mode == ViewMode::Map //&& is_update
                     && let Some(current_position) = self.current_position
                     && let (Some(tiles), Some(memory)) = (&mut self.tiles, &mut self.map_memory)
                     && let (Some(position), _) = find_closest_point(&self.gpx_track, updated_distance)
               {
                  let previous_position = match self.previous_position
                  {
                     | Some(pos) => pos,
                     | None => current_position,
                  };
                  // println!("Displaying map at position: {},{}", position.point.lon, position.point.lat);
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
               else  if self.current_mode == ViewMode::StreetView
               {
                  if self.encrypted_api_key.is_none()
                  {
                     display_streetview_info(ui);
                  }
                  else  if self.gpx_file.is_some() && (is_update || self.is_first_street_frame)
                           && let Some(current_position) = self.current_position
                           && let (Some(position), _) = find_closest_point(&self.gpx_track, updated_distance)
                  {
                     let available_size = ui.available_size();
                     let mut errmsg = String::new();
                     println!("Streetview: {:.4} {:.4} {:.4}", updated_distance, self.current_distance,  requested_delta);

                     let streetview_image = match streetview(ctx, self.encrypted_api_key.as_ref().unwrap(),
                        &current_position, available_size.x, available_size.y, true, true)
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
                        if self.streetview_texture.is_some()
                        {
                           self.streetview_texture.as_mut().unwrap().set(color_image, egui::TextureOptions::LINEAR)
                        }
                        else
                        {
                           self.streetview_texture = Some(ctx.load_texture(texture_name, color_image, Default::default() ));
                        }
                     }
                     else
                     {
                        ui.add(egui::Label::new(
                              egui::RichText::new(errmsg).strong().color(egui::Color32::RED) ));
                     }

                     if let Some(texture) = &self.streetview_texture
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
                     self.previous_position = self.current_position;
                     self.current_position = Some(position);
                     self.current_distance = updated_distance;
                     self.is_first_street_frame = false;
                  } else if self.gpx_file.is_some()
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
               else if  self.current_mode == ViewMode::Gradient  && (is_update || self.is_first_gradient_frame)
                        // && let Some(current_position) = self.current_position
                        && let (Some(position), _) = find_closest_point(&self.gpx_track, updated_distance)
               {
                  // println!("Gradient: {:.4} {:.4} {:.4}", updated_distance, self.current_distance,  requested_delta);
                  let available_size = ui.available_size();
                  let track = self.gpx_track.clone();
                  let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
                  let settings_lock = settings.lock();
                  let gradient_length = settings_lock.gradient_length;
                  let gradient_position = settings_lock.gradient_position;
                  let flat_gradient = settings_lock.flat_gradient_percentage;
                  let extreme_gradient = settings_lock.extreme_gradient_percentage;
                  let mut errmsg = String::new();
                  let   gradient_image = match gradient_image(ctx, &position, track, available_size.x, available_size.y,
                     gradient_position, gradient_length, flat_gradient, extreme_gradient, 1000.0, self.total_distance)
                     {
                        | Ok(img) => Some(img),
                        | Err(msg) =>
                        {
                           eprintln!("Error fetching Street View image: {msg}");
                           errmsg = msg;
                           None

                        }
                     };
                     if let Some(color_image) = gradient_image
                     {
                        // save_tmp_image(&color_image);
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
                        ui.add(egui::Label::new(
                              egui::RichText::new(errmsg).strong().color(egui::Color32::RED) ));
                     }

                     if let Some(texture) = &self.gradient_texture
                     {
                        // println!("Texture size: {:?})", texture.size());
                        ui.centered_and_justified(|ui|
                        {
                           // let img = Image::new(&self.gradient_texture);
                           // ui.image(texture);
                           ui.add(Image::new(texture)
                                    .maintain_aspect_ratio(true)
                                    .fit_to_original_size(1.0)
                                    .shrink_to_fit()
                                 );
                        });
                     }
                     self.previous_position = self.current_position;
                     self.current_position = Some(position);
                     self.current_distance = updated_distance;
                     self.is_first_gradient_frame = false;
               } else if self.gpx_file.is_some()
                     && let Some(texture) = &self.gradient_texture
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
            }
         });

         // Show settings dialog if open
         self.show_settings_dialog(ctx);

         // Show toast notifications
         self.toast_manager.show(ctx);

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

/// Load an embedded PNG image as ColorImage
fn load_embedded_png(asset_name: &str) -> Result<ColorImage, String>
//--------------------------------------------------------------------
{
   let png_data = ASSETS_DIR
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

fn display_invalid_broadcast_directory(ui: &mut egui::Ui, is_aged: bool)
//----------------------------------------------------
{
   let broadcast_file = match get_broadcast_file()
   {
      | Some(dir) => dir,
      | None => PathBuf::from(""),
   };
   let age_msg = if is_aged
   {
      " or the broadcast file has not been updated recently "
   }
   else
   {
      ""
   };
   let errmsg = format!("Could not find a valid TrainingPeaks Virtual broadcast file{}at {:#?}", age_msg, broadcast_file);

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
      ui.add(egui::Label::new( egui::RichText::new(errmsg)
               .strong().color(egui::Color32::RED)));
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
         let track_data: Vec<TrackPoint> = match process_gpx(file_path_disp.to_string().clone().as_str(), DistanceMethod::ECEF)
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

/// Rasterize an SVG from embedded asset data
pub fn rasterize_svg_from_bytes(svg_data: &[u8], width: u32, height: u32) -> Result<ColorImage, String>
//------------------------------------------------------------------------------------------------------
{
   let tree = usvg::Tree::from_data(svg_data, &usvg::Options::default()).map_err(|e| format!("Failed to parse SVG: {}", e))?;

   // Create a pixmap for rendering
   let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| "Failed to create pixmap".to_string())?;

   // Calculate the transform to fit the SVG into the desired size
   let svg_size = tree.size();
   let scale_x = width as f32 / svg_size.width();
   let scale_y = height as f32 / svg_size.height();
   let scale = scale_x.min(scale_y);

   let transform = tiny_skia::Transform::from_scale(scale, scale);

   resvg::render(&tree, transform, &mut pixmap.as_mut());

   // Convert pixmap to egui ColorImage
   let pixels = pixmap.data();
   let mut rgba_pixels = Vec::with_capacity((width * height * 4) as usize);

   // tiny_skia uses premultiplied RGBA, egui expects non-premultiplied RGBA
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

   Ok(ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba_pixels))
}

/// Load an SVG texture from embedded assets
pub fn load_svg_texture(ctx: &Context, name: &str, asset_name: &str, width: u32, height: u32) -> Result<TextureHandle, String>
//----------------------------------------------------------------------------------------------------------------------------
{
   let svg_data = ASSETS_DIR
      .get_file(asset_name)
      .ok_or_else(|| format!("Failed to find embedded asset: {}", asset_name))?
      .contents();

   let color_image = rasterize_svg_from_bytes(svg_data, width, height)?;

   Ok(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
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
fn draw_distance_labels(pixmap: &mut tiny_skia::Pixmap, segment_start_distance: f64, segment_end_distance: f64,
                        label_width: f64, padding: f32, plot_width: f32, plot_height: f32)
//---------------------------------------------------------------------------------------------------------------
{
    use fontdue::{Font, FontSettings};

    // Embedded font data (using a simple fallback)
    const FONT_DATA: &[u8] = include_bytes!("../assets/Roboto-Regular.ttf");

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

#[allow(clippy::too_many_arguments)]
fn gradient_image(ctx: &Context, position: &TrackPoint, track: Arc<Vec<TrackPoint>>,  width: f32, height: f32, gradient_start: f64, gradient_length: f64,
   flat_gradient: f64, extreme_gradient:f64, label_width: f64, total_distance: f64) ->
   Result<ColorImage, String>
//----------------------------------------------------------------------------------------------------------------------------------
{
    use tiny_skia::{Pixmap, Paint, PathBuilder, Stroke, Transform, FillRule};

    // Calculate segment boundaries
    let segment_start_distance = (position.distance - gradient_start).max(0.0);
    let segment_end_distance = (position.distance + gradient_length).min(total_distance);

    //let mut segment_points: Vec<TrackPoint> = Vec::new();
    let mut is_seg_loaded = false;
    let mut segment_points: Vec<TrackPoint> = vec![];
    let i: i64;
    (_, i) = find_closest_point(&track, segment_start_distance);
    if i >= 0
    {
         let j: i64;
         (_, j) = find_closest_point(&track, segment_end_distance);
         if j >= i
         {
            segment_points = track[i as usize ..= j as usize].to_vec();
            is_seg_loaded = true;
         }
    }
    if ! is_seg_loaded
    {
       segment_points = Vec::new();
       for point in track.iter() {
           if point.distance >= segment_start_distance && point.distance <= segment_end_distance {
               segment_points.push(*point);
           }
       }
    }

    if segment_points.len() < 2
    {
        return Err("Insufficient points in segment".to_string());
    }

    // Find min/max elevation for scaling
    let min_elevation = segment_points.iter().map(|p| p.altitude).fold(f64::INFINITY, f64::min);
    let max_elevation = segment_points.iter().map(|p| p.altitude).fold(f64::NEG_INFINITY, f64::max);
    let elevation_range = (max_elevation - min_elevation).max(10.0); // Minimum 10m range

    // Create pixmap with some padding
    let padding = 60.0;
    let plot_width = width - 2.0 * padding;
    let plot_height = height - 2.0 * padding;

    let pixmap_width = width as u32;
    let pixmap_height = height as u32;
    let mut pixmap = Pixmap::new(pixmap_width, pixmap_height)
        .ok_or_else(|| "Failed to create pixmap".to_string())?;

    pixmap.fill(tiny_skia::Color::from_rgba8(224, 224, 224, 255)); ////BGRA  Skyblue (253, 221, 212, 255) #f0f0f0 to #e0e0e0 or #1e1e1e - #2b2b2b (dark theme) or #222831 - #2a2f3a

    // Helper function to map distance and elevation to screen coordinates
    let distance_range = segment_end_distance - segment_start_distance;
    let map_to_screen = |dist: f64, elev: f64| -> (f32, f32) {
        let x = padding as f64 + ((dist - segment_start_distance) / distance_range) * plot_width as f64;
        let y = padding as f64 + plot_height as f64 - ((elev - min_elevation) / elevation_range) * plot_height as f64;
        (x as f32, y as f32)
    };

    // Calculate gradient percentage between two points
    let calculate_gradient_percent = |p1: &TrackPoint, p2: &TrackPoint| -> f64 {
        let horizontal_dist = p2.distance - p1.distance;
        if horizontal_dist < 0.1 { return 0.0; }
        let vertical_dist = p2.altitude - p1.altitude;
        (vertical_dist / horizontal_dist) * 100.0
    };

    // Get color based on gradient percentage
    let gradient_color = |gradient_pct: f64| -> tiny_skia::Color
    {
      //   print!("Gradient pct: {:.2}%", gradient_pct);
        if gradient_pct < flat_gradient.abs() {
            // Downhill: light blue to dark blue
            // let t = ((gradient_pct - flat_gradient.abs()) / extreme_gradient.abs()).min(1.0);
            let t = ((flat_gradient.abs() - gradient_pct) / extreme_gradient.abs()).abs().min(1.0);
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
            //   println!(" (extreme uphill)" );
              tiny_skia::Color::from_rgba8(0, 0, 0, 255)
           }
           else
           {
              // Uphill: light yellow (0.8%) to red (12%+)
            //   println!(" (uphill)" );
              let t = ((gradient_pct - flat_gradient.abs()) / extreme_gradient.abs()).min(1.0);
              let b = (255.0) as u8;
              let g = (255.0 * (1.0 - t)) as u8;
              let r = (150.0 * (1.0 - t)) as u8;
              tiny_skia::Color::from_rgba8(r, g, b, 255)
           }
        }
        else
        {
            // Flat: green
            println!(" (flat)" );
            tiny_skia::Color::from_rgba8(50, 200, 50, 255)
        }
    };

    // Draw filled areas and profile line
    for i in 0..segment_points.len() - 1 {
        let p1 = &segment_points[i];
        let p2 = &segment_points[i + 1];

        let gradient_pct = calculate_gradient_percent(p1, p2);
        let color = gradient_color(gradient_pct);

        let (x1, y1) = map_to_screen(p1.distance, p1.altitude);
        let (x2, y2) = map_to_screen(p2.distance, p2.altitude);

        // Draw filled polygon below the profile
        let bottom_y = (padding + plot_height);
        let mut path_builder = PathBuilder::new();
        path_builder.move_to(x1, y1);
        path_builder.line_to(x2, y2);
        path_builder.line_to(x2, bottom_y);
        path_builder.line_to(x1, bottom_y);
        path_builder.close();

        if let Some(path) = path_builder.finish() {
            let mut paint = Paint::default();
            paint.set_color(color);
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }

        // Draw profile line segment
        let mut path_builder = PathBuilder::new();
        path_builder.move_to(x1, y1);
        path_builder.line_to(x2, y2);

        if let Some(path) = path_builder.finish() {
            let mut paint = Paint::default();
            paint.set_color(color);
            paint.anti_alias = true;
            let stroke = Stroke { width: 3.0, ..Default::default() };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    // Draw current position marker
    if let Some(current_point) = segment_points.iter().find(|p| (p.distance - position.distance).abs() < 1.0) {
        let (marker_x, marker_y) = map_to_screen(current_point.distance, current_point.altitude);

        // Draw arrow similar to DirectionalArrow
        let arrow_size = 15.0;
        let mut path_builder = PathBuilder::new();
        path_builder.move_to(marker_x, marker_y - arrow_size); // Top
        path_builder.line_to(marker_x - arrow_size * 0.6, marker_y + arrow_size * 0.5); // Bottom left
        path_builder.line_to(marker_x + arrow_size * 0.6, marker_y + arrow_size * 0.5); // Bottom right
        path_builder.close();

        if let Some(path) = path_builder.finish() {
            let mut paint = Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(255, 100, 100, 255));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

            // Draw outline
            let stroke = Stroke { width: 2.0, ..Default::default() };
            paint.set_color(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }

        // Draw circle at marker position
        let mut path_builder = PathBuilder::new();
        path_builder.push_circle(marker_x, marker_y, 5.0);

        if let Some(path) = path_builder.finish() {
            let mut paint = Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(255, 128, 128, 255));
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    // Draw distance labels along the bottom axis
    draw_distance_labels(&mut pixmap, segment_start_distance, segment_end_distance,
                         label_width, padding, plot_width, plot_height);

    // Convert pixmap to ColorImage
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

    Ok(ColorImage::from_rgba_unmultiplied([pixmap_width as usize, pixmap_height as usize], &rgba_pixels))
}

fn save_tmp_image(color_image: &ColorImage)
//------------------------------------------------------
{
    match NamedTempFile::new()
   {
      | Ok(tempfile) =>
      {
         let image_path = tempfile.path().to_string_lossy().to_string() + "_streetview_debug.png";
         if let Err(e) = save_image(&color_image, image_path.clone())
         {
            eprintln!("Failed to save debug image: {}", e);
         }
         else
         {
            println!("Saved debug image: {}", image_path);
            println!("Debug: Image dimensions: {}x{}", color_image.size[0], color_image.size[1]);
            println!("Debug: First pixel RGBA: {:?}", color_image.pixels.first());
         }
      }
      | Err(e) =>
      {
         eprintln!("Failed to create temporary file for debug image: {}", e);
      }
   }
}

fn save_image(color_image: &ColorImage, path: String) -> Result<(), String>
//-----------------------------------------------------------------------------------
{
   // Convert ColorImage to image::RgbaImage
   let width = color_image.size[0] as u32;
   let height = color_image.size[1] as u32;
   let pixels: Vec<u8> = color_image.pixels.iter()
      .flat_map(|p| [p.r(), p.g(), p.b(), p.a()])
      .collect();

   let img = image::RgbaImage::from_raw(width, height, pixels)
      .ok_or_else(|| "Failed to create image from ColorImage".to_string())?;

   img.save(&path).map_err(|e| format!("Failed to save image: {}", e))?;
   Ok(())
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

fn get_broadcast_directory() -> Option<PathBuf>
//---------------------------------------------
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

fn get_broadcast_file() -> Option<PathBuf>
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
fn read_rider_data(parse_retries: i64, retry_duration: Duration) -> Option<RiderDataJSON>
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
