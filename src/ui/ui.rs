use std::{
   collections::HashMap,
   path::PathBuf,
   sync::{
      Arc,
      atomic::{AtomicBool, Ordering},
      mpsc::{Receiver, Sender, channel},
   },
   time::Duration,
};

use tempfile::NamedTempFile;
use crossbeam::atomic::AtomicCell;
use tiny_skia::{Pixmap, Paint, PathBuilder, Stroke, Transform, FillRule};

use chrono::{Local, DateTime};
use eframe::{
   CreationContext,
   egui::{self, ColorImage, Context, Image, TextureHandle },
};
use walkers::{HttpTiles, MapMemory, sources::OpenStreetMap};
use include_dir::{include_dir, Dir};

use crate::{
   STARTUP_PARAMS,
   components::ToastManager,
   data::{INVALID_COORDINATE, RiderData, RiderDataJSON },
   gpx::{ TrackPoint, WGS84Position, find_closest_point }, ui::frame::{color_from_gradient, streetview},
};
use crate::SETTINGS;
use crate::settings::Settings;
use crate::ut;

// Minimum average gradient (%) to display the "distance to summit" marker
const MIN_ELEVATION_CHANGE: f64 = 5.0;

// Embed the entire assets directory at compile time
pub(crate) static ASSETS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets");

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ViewMode {
   NA,
   Map,
   StreetView,
   Gradient,
}

const MENU_HEIGHT: u32 = 48;

pub struct GPXAssistUI
//====================
{
   pub(crate) current_mode: Arc<AtomicCell<ViewMode>>,
   pub(crate) toast_manager: ToastManager,
   pub(crate) encrypted_api_key: Option<String>,
   pub(crate) is_first_map_frame: bool,
   pub(crate) is_first_street_frame: bool,
   pub(crate) is_first_gradient_frame: bool,
   pub(crate) gpx_file: Option<PathBuf>,
   pub(crate) gpx_track: Arc<Vec<TrackPoint>>,
   pub(crate) track_total_distance: f64,
   pub(crate) gradient_distance: i64,
   pub(crate) requested_delta: Arc<AtomicCell<i64>>,
   pub(crate) simulated_speed: Arc<AtomicCell<f64>>,
   pub(crate) textures: HashMap<String, (TextureHandle, [f32; 2])>,
   pub(crate) previous_gpx_position: Option<TrackPoint>,
   pub(crate) open_dialog_channel: (Sender<(Vec<TrackPoint>, String, String)>, Receiver<(Vec<TrackPoint>, String, String)>),
   pub(crate) tiles: Option<HttpTiles>,
   pub(crate) map_memory: Option<MapMemory>,
   pub(crate) streetview_texture: Option<TextureHandle>,

   pub(crate) gradient_start: i64,
   pub(crate) gradient_end: i64,
   pub(crate) gradient_points: Vec<TrackPoint>, // = vec![]
   pub(crate) gradient_texture: Option<TextureHandle>,
   pub(crate) gradient_length: Arc<AtomicCell<i64>>,
   pub(crate) gradient_offset: Arc<AtomicCell<i64>>,
   pub(crate) gradient_delta: Arc<AtomicCell<i64>>,
   pub(crate) gradient_flat: Arc<AtomicCell<f64>>,
   pub(crate) gradient_medium: Arc<AtomicCell<f64>>,
   pub(crate) gradient_extreme: Arc<AtomicCell<f64>>,
   pub(crate) vertical_scale: Arc<AtomicCell<f64>>,
   pub(crate) gradient_pixmap: Option<Box<Pixmap>>,
   pub(crate) gradient_pixmap_width: u32,
   pub(crate) gradient_pixmap_height: u32,

   pub(crate) is_simulating: Arc<AtomicBool>,
   pub(crate) must_stop_thread: Arc<AtomicBool>,
   pub(crate) distance_thread_handle: Option<std::thread::JoinHandle<()>>,
   pub(crate) simulation_thread_handle: Option<std::thread::JoinHandle<()>>,

   pub(crate) rider_data: Arc<AtomicCell<RiderData>>,

   pub show_settings_dialog: bool,
   pub show_settings_dialog_err: bool,
   pub settings_dialog_message: String,
}

impl Default for GPXAssistUI
//===========================
{
   fn default() -> Self
//------------------
   {
      let cmdline_opts = STARTUP_PARAMS.lock();
      let cmdline_opts = cmdline_opts.borrow();
      let filepath_opt = cmdline_opts
         .as_ref()
         .and_then(|opts| opts.file_path.as_ref().map(|s| PathBuf::from(s)));
      let tiles_opt: Option<HttpTiles> = None;
      let map_memory_opt: Option<MapMemory> = None;

      let mut gradient_length: i64;
      let mut gradient_offset: i64;
      let mut flat_gradient: f64;
      let mut medium_gradient: f64;
      let mut extreme_gradient: f64;
      let mut vertical_exaggeration: f64;
      let mut api_key: Option<String>;
      {
         let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
         let settings_lock = settings.lock();
         gradient_length = settings_lock.gradient_length.round() as i64;
         if gradient_length <= 0 || gradient_length >= 20000
         {
            gradient_length = 3000;
         }
         gradient_offset = settings_lock.gradient_offset.round() as i64;
         if gradient_offset < 0 || gradient_offset >= gradient_length
         {
            gradient_offset = 100
         }
         flat_gradient = settings_lock.flat_gradient_percentage;
         if flat_gradient < 0.0 || flat_gradient >= 5.0
         {
            flat_gradient = 1.0;
         }
         medium_gradient = settings_lock.medium_gradient_percentage;
         if medium_gradient < 0.0 || medium_gradient >= 50.0
         {
            medium_gradient = 8.0;
         }
         extreme_gradient = settings_lock.extreme_gradient_percentage;
         if extreme_gradient < 5.0 || extreme_gradient > 100.0
         {
            extreme_gradient = 16.0;
         }
         vertical_exaggeration = settings_lock.vertical_exaggeration;
         if vertical_exaggeration < 1.0 || vertical_exaggeration > 50.0
         {
            vertical_exaggeration = 10.0;
         }
         api_key = settings_lock.get_streetview_api_key().ok();
         if api_key.is_some() && api_key.as_ref().unwrap().is_empty()
         {
            api_key = None
         }
      }

      Self {
         current_mode: Arc::new(AtomicCell::new(ViewMode::NA)),
         toast_manager: ToastManager::new(),
         encrypted_api_key: api_key,
         is_first_map_frame: true,
         is_first_street_frame: true,
         is_first_gradient_frame: true,
         gpx_file: filepath_opt,
         gpx_track: Arc::new(vec![]),
         track_total_distance: 0.0,
         previous_gpx_position: None,
         requested_delta: Arc::new(AtomicCell::new(100)),
         simulated_speed: Arc::new(AtomicCell::new(45.0)),
         textures: HashMap::new(),
         open_dialog_channel: channel(),
         tiles: tiles_opt,
         map_memory: map_memory_opt,
         streetview_texture: None,
         gradient_start: 0,
         gradient_end: 0,
         gradient_texture: None,
         gradient_points: vec![],
         gradient_length: Arc::new(AtomicCell::new(gradient_length)),
         gradient_offset: Arc::new(AtomicCell::new(gradient_offset)),
         gradient_delta: Arc::new(AtomicCell::new(10)),
         gradient_flat: Arc::new(AtomicCell::new(flat_gradient)),
         gradient_medium: Arc::new(AtomicCell::new(medium_gradient)),
         gradient_extreme: Arc::new(AtomicCell::new(extreme_gradient)),
         vertical_scale: Arc::new(AtomicCell::new(vertical_exaggeration)),
         gradient_distance: 0,
         gradient_pixmap: None,
         gradient_pixmap_width: 0,
         gradient_pixmap_height: 0,
         is_simulating: Arc::new(AtomicBool::new(false)),
         must_stop_thread: Arc::new(AtomicBool::new(false)),
         distance_thread_handle: None,
         simulation_thread_handle: None,
         rider_data: Arc::new(AtomicCell::new(RiderData::default())),
         show_settings_dialog: false,
         show_settings_dialog_err: false,
         settings_dialog_message: String::new(),
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
         | Ok(texture) => {
            app.textures
               .insert("open".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) => {
            eprintln!("Failed to load open icon texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "test_on_icon", "test_icon.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) => {
            app.textures
               .insert("test-on".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) => {
            eprintln!("Failed to load test icon texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "test_off_icon", "test_off_icon.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) => {
            app.textures
               .insert("test-off".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) => {
            eprintln!("Failed to load test off icon texture {e}.");
         }
      }

      match load_svg_texture(&cc.egui_ctx, "map_on_icon", "globe-on.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) => {
            app.textures
               .insert("map-on".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) => {
            eprintln!("Failed to load map on texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "map_off_icon", "globe-off.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) => {
            app.textures
               .insert("map-off".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) => {
            eprintln!("Failed to load map off texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "street_on_icon", "streetview-on.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) => {
            app.textures
               .insert("street-on".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) => {
            eprintln!("Failed to load streetview on icon texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "street_off_icon", "streetview-off.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) => {
            app.textures
               .insert("street-off".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) => {
            eprintln!("Failed to load streetview off icon texture {e}.");
         }
      }
      match load_svg_texture(&cc.egui_ctx, "settings_icon", "settings.svg", MENU_HEIGHT, MENU_HEIGHT)
      {
         | Ok(texture) => {
            app.textures
               .insert("settings".to_string(), (texture, [MENU_HEIGHT as f32, MENU_HEIGHT as f32]));
         }
         | Err(e) => {
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

      app.is_first_map_frame = true;
      app.start_distance_thread(&cc.egui_ctx);
      app
   }

   pub(crate) fn start_distance_thread(&mut self, context: &Context)
   //--------------------------------------------
   {
      if let Some(handle) = self.distance_thread_handle.take()
      {
         self.must_stop_thread.store(true, Ordering::Relaxed);
         std::thread::sleep(Duration::from_millis(100));
         let _ = handle.join();
      }
      let current_mode = self.current_mode.clone();
      let requested_delta = self.requested_delta.clone();
      let gradient_delta = self.gradient_delta.clone();
      let is_simulating = self.is_simulating.clone();
      let track = self.gpx_track.clone();
      let ctx = context.clone();
      let must_stop_thread = self.must_stop_thread.clone();      
      let rider_data_clone = self.rider_data.clone();      
      must_stop_thread.store(false, Ordering::Relaxed);      
      let handle = std::thread::spawn(move || 
      {
         GPXAssistUI::update_distance_thread(
            ctx, requested_delta, gradient_delta, current_mode, track, is_simulating,
            must_stop_thread, rider_data_clone);
      });
      self.distance_thread_handle = Some(handle);
   }

   #[allow(unused)]
   #[allow(clippy::too_many_arguments)]
   pub(crate) fn update_distance_thread(ctx: Context, requested_delta: Arc<AtomicCell<i64>>,
      gradient_delta: Arc<AtomicCell<i64>>, mode: Arc<AtomicCell<ViewMode>>,
      track: Arc<Vec<TrackPoint>>, is_simulating: Arc<AtomicBool>,
      must_stop: Arc<AtomicBool>, rider_data: Arc<AtomicCell<RiderData>>,
   )
   //--------------------------------------------------------------------------------------------------------------------
   {
      let mut last_distance: i64 = 0;
      let mut last_gradient_distance: i64 = 0;
      let mut distance: i64;
      let total_distance = track.last().map_or(0.0, |p| p.distance);
      let tcc= track.len();
      println!("Starting distance update thread. ({tcc} track points loaded)");
      while !must_stop.load(Ordering::Relaxed)
      {
         if is_simulating.load(Ordering::Relaxed)
         {
            std::thread::sleep(Duration::from_secs(1));
            continue;
         }
         let mut rider_json = match super::frame::read_rider_data(3, Duration::from_millis(300))
         {
            | Some(r) => r,
            | None => {
               std::thread::sleep(Duration::from_secs(1));
               continue;
            }
         };

         distance = rider_json.distance as i64;
         if !track.is_empty() && distance as f64 > total_distance // Assume laps
         {                                    
            distance = total_distance as i64 - distance;            
         }
         if last_distance > 0 && distance < last_distance
         {
            // Reset last distances if distance has decreased (e.g., new ride started)
            last_distance = 0;
            last_gradient_distance = 0;
         }
         let gradient_distance_delta = if !track.is_empty() { distance - last_gradient_distance } else { 0 };
         // if mode.load() == ViewMode::Gradient && gradient_distance_delta >= gradient_delta.load()
         // {
         //    last_gradient_distance = distance;
         // }
         let distance_delta = distance - last_distance;
         if distance_delta > 0 || gradient_distance_delta > 0
         {
            if (rider_json.latitude == INVALID_COORDINATE || rider_json.longitude == INVALID_COORDINATE) && !track.is_empty()
            {
               if let (Some(position), _) = find_closest_point(&track, distance as f64)
               {
                  rider_json.latitude = RiderDataJSON::degrees_2_semicircles(position.point.latitude);
                  rider_json.longitude = RiderDataJSON::degrees_2_semicircles(position.point.longitude);
                  rider_json.altitude = (position.point.altitude*1000.0) as i64;
               }
               else
               {
                  rider_json.latitude = INVALID_COORDINATE;
                  rider_json.longitude = INVALID_COORDINATE;
                  rider_json.altitude = INVALID_COORDINATE;
               }
            }

            let mut rider = RiderData::from(rider_json);
            if distance_delta >= requested_delta.load()
            {
               rider.previous_distance = last_distance;
               last_distance = distance;
            }
            if gradient_distance_delta >= gradient_delta.load()
            {
               rider.previous_gradient_distance = last_gradient_distance;
               last_gradient_distance = distance;
            }
            rider_data.store(rider);
            ctx.request_repaint();
         }
         std::thread::sleep(Duration::from_secs(1));
      }
      println!("Distance update thread exiting.");
   }

   pub(crate) fn start_simulation_thread(&mut self, context: &Context)
   //------------------------------------------------------------------------
   {
      if let Some(handle) = self.simulation_thread_handle.take()
      {
         self.is_simulating.store(false, Ordering::Relaxed);
         std::thread::sleep(Duration::from_millis(100));
         let _ = handle.join();
      }
      self.is_simulating.store(true, Ordering::Relaxed);
      let rider_data = self.rider_data.clone();
      let requested_delta = self.requested_delta.clone();
      let gradient_delta = self.gradient_delta.clone();
      let simulated_speed = self.simulated_speed.clone();
      let is_simulating = self.is_simulating.clone();
      let current_mode = self.current_mode.clone();
      let track = self.gpx_track.clone();
      let total_distance = self.track_total_distance;
      let ctxx = context.clone();
      let handle = std::thread::spawn(move ||
      {
         GPXAssistUI::simulate_movement_thread(ctxx, track, requested_delta, gradient_delta, simulated_speed, rider_data, total_distance,
            current_mode,  is_simulating);
      });
      self.simulation_thread_handle = Some(handle);
   }

   #[allow(clippy::too_many_arguments)]
   #[allow(unused_variables)]
   pub(crate) fn simulate_movement_thread( ctx: Context, track: Arc<Vec<TrackPoint>>, requested_delta: Arc<AtomicCell<i64>>,
      gradient_delta: Arc<AtomicCell<i64>>, simulated_speed: Arc<AtomicCell<f64>>, rider_data: Arc<AtomicCell<RiderData>>, total_distance: f64,
      mode: Arc<AtomicCell<ViewMode>>, is_simulating: Arc<AtomicBool> )
   //-------------------------------------------------------------------------------------------------
   {
      let mut distance: i64 = 0;
      let mut last_gradient_distance: i64 = 0;
      let mut distance_delta = requested_delta.load();
      let mut last_distance: i64 = -distance_delta;      
      //let speed: f64 = 45.0 * 1000.0 / (60.0 * 60.0); // km/h to m/s
      let speed = simulated_speed.load() * 1000.0 / (60.0 * 60.0); 
      let start: DateTime<Local> = Local::now();
      while distance < total_distance as i64
      {
         if !is_simulating.load(Ordering::Relaxed)
         {
            break;
         }
         if (distance - last_distance) >= distance_delta
         {
            let mut rider = RiderData
            {
               distance,
               previous_distance: last_distance,
               ..Default::default()
            }; //::default();
            // rider.distance = distance as i32;
            if let (Some(position), _) = find_closest_point(&track, distance as f64)
            {
               rider.latitude = position.point.latitude;
               rider.longitude = position.point.longitude;
               rider.altitude = position.point.altitude;
            }
            rider.wind_speed = 10;
            rider.wind_angle = 60;
            rider_data.store(rider);
            last_distance = distance;
            ctx.request_repaint();
            // println!("Simulated distance: {:.2} meters ({:.2}km)", distance, distance / 1000.0);
         }
         if mode.load() == ViewMode::Gradient && (distance - last_gradient_distance) >= gradient_delta.load()
         {
            // last_gradient_distance = distance;
            let mut rider = RiderData 
            {
               distance,
               previous_distance: last_distance,
               ..Default::default()
            };
            if let (Some(position), _) = find_closest_point(&track, distance as f64) {
               rider.latitude = position.point.latitude;
               rider.longitude = position.point.longitude;
               rider.altitude = position.point.altitude;
            }
            rider.wind_speed = 10;
            rider.wind_angle = 60;
            rider_data.store(rider);
            last_gradient_distance = distance;
            ctx.request_repaint();
            // println!("Sent gradient distance: {:.2} meters ({:.2}km)", distance, distance / 1000);
         }

         let now: DateTime<Local> = Local::now();
         let total_time = (now - start).num_seconds() as f64;
         distance = (speed * total_time).round() as i64;

         if !is_simulating.load(Ordering::Relaxed) {
            break;
         }
         std::thread::sleep(Duration::from_secs(1));
         distance_delta = requested_delta.load();
      }
      is_simulating.store(false, Ordering::Relaxed);
      // println!("Simulation thread exiting.");
   }

   pub(crate) fn check_broadcast_file(&mut self) -> (bool, bool)
//----------------------------------
   {
      let broadcast_file = super::frame::get_broadcast_file();
      let is_exists = broadcast_file.is_some() && broadcast_file.as_ref().unwrap().is_file();
      let mut age: chrono::Duration = chrono::Duration::zero();
      if is_exists {
         age = match ut::get_file_age(broadcast_file.as_ref().unwrap()) {
            | Ok(d) => d,
            | Err(e) => {
               eprintln!("Error getting broadcast file age: {}", e);
               chrono::Duration::zero()
            }
         };
      }
      let is_aged = age.num_minutes() > 1;
      (is_exists, is_aged)
   }

   pub(crate) fn display_streetview(&mut self, ctx: &Context, ui: &mut egui::Ui, previous_position: &WGS84Position, position: &WGS84Position,
      fov: i64)
   //-----------------------------------------------------------------------------------------------------------------------
   {
      if self.encrypted_api_key.is_some()
      {
         let available_size = ui.available_size();
         let mut errmsg = String::new();

         let streetview_image =
            match streetview(self.encrypted_api_key.as_ref().unwrap(), &previous_position,  &position, available_size.x, available_size.y, fov, true, true)
            {
               | Ok(img) => Some(img),
               | Err(msg) => {
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
               self.streetview_texture = Some(ctx.load_texture(texture_name, color_image, Default::default()));
            }
         }
         else
         {
            ui.add(egui::Label::new(
               egui::RichText::new(errmsg)
                  .strong()
                  .color(egui::Color32::RED),
            ));
         }

         if let Some(texture) = &self.streetview_texture
         {
            // println!("Texture size: {:?})", texture.size());
            ui.centered_and_justified(|ui|
            {
               // let img = Image::new(&self.streetview_texture);
               // ui.image(texture);
               ui.add( Image::new(texture)
                        .maintain_aspect_ratio(false)
                        .fit_to_exact_size(available_size)
                        .shrink_to_fit(),
               );
            });
            self.is_first_street_frame = false;
         }
      }
   }

   // #[allow(clippy::too_many_arguments)]
   pub fn new_gradient_image( &mut self, distance: i64, total_distance: f64, width: f32, height: f32, label_width: i64 )
      -> Result<ColorImage, String>
   //----------------------------------------------------------------------------------------------------------------------------------
   {
      let track: Arc<Vec<TrackPoint>> = self.gpx_track.clone();
      let gradient_length = self.gradient_length.load();
      let flat_gradient = self.gradient_flat.load();
      let medium_gradient = self.gradient_medium.load();
      let extreme_gradient = self.gradient_extreme.load();
      let gradient_offset = self.gradient_offset.load();
      let extreme_start = extreme_gradient.abs() - 1.5;

      self.gradient_start = (distance - gradient_offset ).max(0);
      self.gradient_end = (self.gradient_start + gradient_length).min(total_distance as i64);
      if self.gradient_end >= total_distance as i64
      {
         self.gradient_start = (self.gradient_end - gradient_length).max(0);
      }

      //let mut segment_points: Vec<TrackPoint> = Vec::new();
      let mut is_seg_loaded = false;
      let i: i64;
      (_, i) = find_closest_point(&track, self.gradient_start as f64);
      if i >= 0 {
         let j: i64;
         (_, j) = find_closest_point(&track, self.gradient_end as f64);
         if j >= i {
            self.gradient_points = track[i as usize..=j as usize].to_vec();
            is_seg_loaded = true;
         }
      }
      if !is_seg_loaded {
         self.gradient_points = Vec::new();
         for point in track.iter()
         {
            let distance = point.distance as i64;
            if distance >= self.gradient_start && distance <= self.gradient_end
            {
               self.gradient_points.push(*point);
            }
         }
      }

      if self.gradient_points.len() < 2
      {
         return Err("Insufficient points in segment".to_string());
      }

      // Find min/max elevation for scaling
      let min_elevation = self.gradient_points
         .iter()
         .map(|p| p.point.altitude)
         .fold(f64::INFINITY, f64::min);
      let max_elevation_point: Option<&TrackPoint>;
      let max_elevation: f64; //= self .gradient_points.iter().map(|p| p.point.altitude).fold(f64::NEG_INFINITY, f64::max);
      if let Some((_, max_point)) = self.gradient_points.iter().enumerate()
            .max_by(|(_, a), (_, b)| a.point.altitude.partial_cmp(&b.point.altitude).unwrap_or(std::cmp::Ordering::Equal))
      {         
         max_elevation_point = Some(max_point);
         max_elevation =  max_elevation_point.unwrap().point.altitude;
      }
      else
      {
         max_elevation_point = None;
         max_elevation = self .gradient_points.iter().map(|p| p.point.altitude).fold(f64::NEG_INFINITY, f64::max);
      }
      let elevation_range = (max_elevation - min_elevation).max(10.0); // Minimum 10m range to avoid division by near-zero in aspect ratio
      // let average_gradient = (max_elevation - min_elevation) / (self.gradient_end - self.gradient_start) as f64 * 100.0;

      let pixmap_width = width as u32;
      let pixmap_height = height as u32;
      let mut pixmap = Pixmap::new(pixmap_width, pixmap_height).ok_or_else(|| "Failed to create pixmap".to_string())?;

      pixmap.fill(tiny_skia::Color::from_rgba8(224, 224, 224, 255)); ////BGRA  Skyblue (253, 221, 212, 255) #f0f0f0 to #e0e0e0 or #1e1e1e - #2b2b2b (dark theme) or #222831 - #2a2f3a

      let padding = 60.0;
      let plot_width = width - 2.0 * padding;
      let plot_height = height - 2.0 * padding;
      let distance_range = self.gradient_end - self.gradient_start;

      // Calculate proper aspect ratio with vertical exaggeration
      let vertical_exaggeration = self.vertical_scale.load();
      let actual_aspect_ratio = elevation_range / distance_range as f64; // e.g., 50m / 3000m = 0.0167
      let display_aspect_ratio = actual_aspect_ratio * vertical_exaggeration; // e.g., 0.0167 * 10 = 0.167

      // Calculate the effective plot height based on aspect ratio
      // The elevation should be scaled to fit within the available height while maintaining the aspect ratio
      let effective_plot_height = (plot_width * display_aspect_ratio as f32).min(plot_height);
      let elevation_offset = (plot_height - effective_plot_height) / 2.0; // Center vertically

      let map_to_screen = |dist: f64, elev: f64| -> (f32, f32) 
      {
         let x = padding as f64 + ((dist - self.gradient_start as f64) / distance_range as f64) * plot_width as f64;
         let y = padding as f64 + elevation_offset as f64 + effective_plot_height as f64
            - ((elev - min_elevation) / elevation_range) * effective_plot_height as f64;
         (x as f32, y as f32)
      };

      // Calculate gradient percentage between two points
      let calculate_gradient_percent = |p1: &TrackPoint, p2: &TrackPoint| -> f64
      {
         let horizontal_dist = p2.distance - p1.distance;
         if horizontal_dist < 0.1 {
            return 0.0;
         }
         let vertical_dist = p2.point.altitude - p1.point.altitude;
         (vertical_dist / horizontal_dist) * 100.0
      };

      // let _ = std:: fs::remove_file("scripts/gpxdata.csv");
      for i in 0..self.gradient_points.len() - 1
      {
         let p1 = &self.gradient_points[i];
         let p2 = &self.gradient_points[i + 1];

         let gradient_pct = calculate_gradient_percent(p1, p2);
         let color = color_from_gradient(gradient_pct, flat_gradient, medium_gradient, extreme_gradient, extreme_start);

         // Debug logging to CSV
         // use std::fs::OpenOptions;
         // {
         //    match OpenOptions::new().append(true).create(true).open("scripts/gpxdata.csv")
         //    {
         //       | Ok(mut file) =>
         //       {
         //          use std::io::Write;
         //          let mut log_line = format!("{},{},{},{},{},{}\n", p1.distance, p1.altitude, color.red(), color.green(), color.blue(), gradient_pct);
         //          let _ = file.write_all(log_line.as_bytes());
         //          log_line = format!("{},{},{},{},{},{}\n", p2.distance, p2.altitude, color.red(), color.green(), color.blue(), gradient_pct);
         //          let _ = file.write_all(log_line.as_bytes());
         //       }
         //       | Err(e) =>
         //       {
         //          eprintln!("Error writing to log file: {}", e);
         //       }
         //    }
         // }

         let (x1, y1) = map_to_screen(p1.distance, p1.point.altitude);
         let (x2, y2) = map_to_screen(p2.distance, p2.point.altitude);

         // Draw filled polygon below the profile
         let bottom_y = padding + elevation_offset + effective_plot_height;
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
            let stroke = Stroke {
               width: 3.0,
               ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
         }
      }

      super::frame::draw_distance_labels(&mut pixmap, self.gradient_start, self.gradient_end,
                                          label_width, padding, plot_width, plot_height);

      // Draw summit marker if average gradient exceeds threshold
      /*  
         max_elevation_point = Some(max_point);

       */
      // print!("{:.2} - {:.2} = {:.2} ({:.2})", max_elevation, min_elevation, max_elevation - min_elevation, MIN_ELEVATION_CHANGE);
      if let Some(max_point) = max_elevation_point && (max_elevation - min_elevation) >= MIN_ELEVATION_CHANGE
      {
         let max_altitude_point = *max_point;
         let distance_to_summit = (max_altitude_point.distance - distance as f64).max(0.0);

         // println!("  Distance to summit: {:.2} {:.2} {:.2}", distance_to_summit, gradient_length, (gradient_length as f64 - 40.0).max(5.0));
         if distance_to_summit > 0.0 //&& distance_to_summit < (gradient_length as f64 - 40.0).max(5.0)
         {
            // Draw summit marker (diamond shape) at max altitude point
            let (summit_x, mut summit_y) = map_to_screen(max_altitude_point.distance, max_altitude_point.point.altitude);

            summit_y -= 15.0; // Offset above the profile line
            let diamond_size = 12.0;
            let mut path_builder = PathBuilder::new();
            path_builder.move_to(summit_x, summit_y - diamond_size); // Top
            path_builder.line_to(summit_x + diamond_size * 0.7, summit_y); // Right
            path_builder.line_to(summit_x, summit_y + diamond_size); // Bottom
            path_builder.line_to(summit_x - diamond_size * 0.7, summit_y); // Left
            path_builder.close();

            if let Some(path) = path_builder.finish()
            {
               let mut paint = Paint::default();
               paint.set_color(tiny_skia::Color::from_rgba8(255, 215, 0, 255)); // Gold color
               paint.anti_alias = true;
               pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

               // Draw outline
               let stroke = Stroke
               {
                  width: 2.0,
                  ..Default::default()
               };
               paint.set_color(tiny_skia::Color::from_rgba8(0, 0, 0, 255));
               pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }

            // Draw Distance to summit text on top left
            super::frame::draw_summit_info(&mut pixmap, distance_to_summit, padding);
         }
         
      }

      self.gradient_pixmap = Some(Box::new(pixmap.clone()));
      self.gradient_pixmap_width = pixmap_width;
      self.gradient_pixmap_height = pixmap_height;

      if distance >= 0
      {
         match self.draw_gradient_marker(width, height, distance)
         {
            | Ok(img) => return Ok(img),
            | Err(msg) => {
               eprintln!("Error recalculating gradient image: {msg}");
            }
         };
      }
      // Ok((pixmap, pixmap_width, pixmap_height))
      Ok(super::frame::pixmap_to_image(&pixmap, pixmap_width, pixmap_height))
   }

   pub fn draw_gradient_marker( &mut self, width: f32, height: f32, distance: i64) -> Result<ColorImage, String>
   //-----------------------------------------
   {
      if let Some(gradient_pixmap) = &mut self.gradient_pixmap
         && !self.gradient_points.is_empty()
      {
         let search_result = self.gradient_points.binary_search_by(|probe| {
            probe
               .distance
               .partial_cmp(&(distance as f64))
               .unwrap_or(core::cmp::Ordering::Equal)
         });
         let (mut pt, _) = match search_result
         {
            | Ok(index) => (Some(self.gradient_points[index]), index as i64),
            | Err(index) => {
               let chosen_index = if index == 0 {
                  0
               } else if index >= self.gradient_points.len()
               {
                  self.gradient_points.len() - 1
               }
               else
               {
                  let prev = self.gradient_points[index - 1];
                  let next = self.gradient_points[index];
                  if (distance as f64 - prev.distance) <= (next.distance - distance as f64) { index - 1 } else { index }
               };
               (Some(self.gradient_points[chosen_index]), chosen_index as i64)
            }
         };
         if pt.is_none()
         {
            match self.gradient_points
               .iter()
               .find(|p| (distance as f64 - p.distance).abs() < 1.0)
            {
               | Some(p) => pt = Some(*p),
               | None => return Err("Current point not found in gradient points".to_string()),
            }
         }
         if let Some(current_point) = pt
         {
            let mut pixmap = (*gradient_pixmap).clone();
            let padding = 60.0;
            let plot_width = width - 2.0 * padding;
            let plot_height = height - 2.0 * padding;
            let distance_range = self.gradient_end - self.gradient_start;
            let min_elevation = self.gradient_points
               .iter()
               .map(|p| p.point.altitude)
               .fold(f64::INFINITY, f64::min);
            let max_elevation = self.gradient_points
               .iter()
               .map(|p| p.point.altitude)
               .fold(f64::NEG_INFINITY, f64::max);
            let elevation_range = (max_elevation - min_elevation).max(10.0); // Minimum 10m range

            // Calculate proper aspect ratio with vertical exaggeration (same as new_gradient_image)
            let vertical_exaggeration = self.vertical_scale.load();
            let actual_aspect_ratio = elevation_range / distance_range as f64;
            let display_aspect_ratio = actual_aspect_ratio * vertical_exaggeration;
            let effective_plot_height = (plot_width * display_aspect_ratio as f32).min(plot_height);
            let elevation_offset = (plot_height - effective_plot_height) / 2.0;

            let map_to_screen = |dist: f64, elev: f64| -> (f32, f32) {
               let x = padding as f64 + ((dist - self.gradient_start as f64) / distance_range as f64) * plot_width as f64;
               let y = padding as f64 + elevation_offset as f64 + effective_plot_height as f64
                  - ((elev - min_elevation) / elevation_range) * effective_plot_height as f64;
               (x as f32, y as f32)
            };
            let (marker_x, marker_y) = map_to_screen(current_point.distance, current_point.point.altitude);

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
               let stroke = Stroke
               {
                  width: 2.0,
                  ..Default::default()
               };
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
            Ok(super::frame::pixmap_to_image(&pixmap, self.gradient_pixmap_width, self.gradient_pixmap_height))
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

#[cfg(debug_assertions)]
#[allow(dead_code)]
fn save_tmp_image(color_image: &ColorImage)
//------------------------------------------------------
{
   match NamedTempFile::new() {
      | Ok(tempfile) => {
         let image_path = tempfile.path().to_string_lossy().to_string() + "_streetview_debug.png";
         if let Err(e) = save_image(&color_image, image_path.clone()) {
            eprintln!("Failed to save debug image: {}", e);
         } else {
            println!("Saved debug image: {}", image_path);
            println!("Debug: Image dimensions: {}x{}", color_image.size[0], color_image.size[1]);
            println!("Debug: First pixel RGBA: {:?}", color_image.pixels.first());
         }
      }
      | Err(e) => {
         eprintln!("Failed to create temporary file for debug image: {}", e);
      }
   }
}

#[cfg(debug_assertions)]
fn save_image(color_image: &ColorImage, path: String) -> Result<(), String>
//-----------------------------------------------------------------------------------
{
   // Convert ColorImage to image::RgbaImage
   let width = color_image.size[0] as u32;
   let height = color_image.size[1] as u32;
   let pixels: Vec<u8> = color_image
      .pixels
      .iter()
      .flat_map(|p| [p.r(), p.g(), p.b(), p.a()])
      .collect();

   let img = image::RgbaImage::from_raw(width, height, pixels).ok_or_else(|| "Failed to create image from ColorImage".to_string())?;

   img.save(&path)
      .map_err(|e| format!("Failed to save image: {}", e))?;
   Ok(())
}

pub fn get_broadcast_directory_or_default() -> PathBuf
//---------------------------------------------
{
   if cfg!(target_os = "macos") {
      // ~/TPVirtual/Broadcast/focus.json
      match dirs::home_dir() {
         | Some(dir) => dir.join("TPVirtual").join("Broadcast").clone(),
         | None => PathBuf::new(),
      }
   } else {
      match dirs::document_dir() {
         | Some(dir) => dir.join("TPVirtual").join("Broadcast").clone(),
         | None => PathBuf::new(),
      }
   }
}
