use std::{collections::HashMap, fs::OpenOptions, path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{Receiver, Sender, channel}}, time::Duration};

use tempfile::NamedTempFile;
use crossbeam::atomic::AtomicCell;
use tiny_skia::{Pixmap, Paint, PathBuilder, Stroke, Transform, FillRule};

use chrono::{Local, DateTime};
use eframe::{CreationContext, egui::{self, Color32, ColorImage, Context, Image, TextureHandle, Vec2}, emath::Numeric};
use walkers::{HttpTiles, Map, MapMemory, lon_lat, sources::OpenStreetMap};
use include_dir::{include_dir, Dir};

use crate::{ STARTUP_PARAMS, components::{self, DirectionalArrow, ToastManager}, data::{RiderData, RiderDataJSON}, gpx::{ TrackPoint, find_closest_point, process_gpx } };
use crate::SETTINGS;
use crate::settings::Settings;
use crate::ut;

// Embed the entire assets directory at compile time
pub(crate) static ASSETS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets");

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
   pub(crate) current_mode:                  Arc<AtomicCell<ViewMode>>,
   pub(crate) toast_manager:                 ToastManager,
   pub(crate) encrypted_api_key:             Option<String>,
   pub(crate) is_first_map_frame:            bool,
   pub(crate) is_first_street_frame:         bool,
   pub(crate) is_first_gradient_frame:       bool,
   pub(crate) gpx_file:                      Option<PathBuf>,
   pub(crate) gpx_track:                     Arc<Vec<TrackPoint>>,
   pub(crate) total_distance:                f64,
   pub(crate) current_distance:              f64,
   pub(crate) gradient_distance:             f64,
   pub(crate) updated_distance:              Arc<AtomicCell<f64>>,
   pub(crate) requested_delta:               Arc<AtomicCell<f64>>,
   pub(crate) simulated_speed:               Arc<AtomicCell<f64>>,
   pub(crate) textures:                      HashMap<String, (TextureHandle, [f32; 2])>,
   pub(crate) previous_position:             Option<TrackPoint>,
   pub(crate) current_position:              Option<TrackPoint>,
   pub(crate) open_dialog_channel:           (Sender<(Vec<TrackPoint>, String)>, Receiver<(Vec<TrackPoint>, String)>),
   pub(crate) tiles:                         Option<HttpTiles>,
   pub(crate) map_memory:                    Option<MapMemory>,
   pub(crate) streetview_texture:            Option<TextureHandle>,

   pub(crate) gradient_start:                f64,
   pub(crate) gradient_end:                  f64,
   pub(crate) gradient_points:               Vec<TrackPoint>, // = vec![]
   pub(crate) gradient_texture:              Option<TextureHandle>,
   pub(crate) gradient_length:               Arc<AtomicCell<f64>>,
   pub(crate) gradient_offset:               Arc<AtomicCell<f64>>,
   pub(crate) gradient_delta:                Arc<AtomicCell<f64>>,
   pub(crate) gradient_flat:                 Arc<AtomicCell<f64>>,
   pub(crate) gradient_extreme:              Arc<AtomicCell<f64>>,
   pub(crate) vertical_scale:                Arc<AtomicCell<f64>>,
   pub(crate) gradient_pixmap:               Option<Box<Pixmap>>,
   pub(crate) gradient_pixmap_width:         u32,
   pub(crate) gradient_pixmap_height:        u32,
   pub(crate) is_simulating:                 Arc<AtomicBool>,
   pub(crate) is_running:                    Arc<AtomicBool>,
   pub(crate) rider_data:                    Arc<AtomicCell<RiderData>>,

   pub show_settings_dialog:     bool,
   pub show_settings_dialog_err: bool,
   pub settings_dialog_message:  String,
}

impl Default for GPXAssistUI
//===========================
{
   fn default() -> Self
//------------------
   {
      let cmdline_opts = STARTUP_PARAMS.lock();
      let cmdline_opts = cmdline_opts.borrow();
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
         let track_data: Vec<TrackPoint> = match process_gpx(&file_path)
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
         current_mode: Arc::new(AtomicCell::new(ViewMode::NA)),
         toast_manager: ToastManager::new(),
         encrypted_api_key: api_key,
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
         previous_position,
         current_position,
         open_dialog_channel: channel(),
         tiles: tiles_opt,
         map_memory: map_memory_opt,
         streetview_texture: None,
         gradient_start:               0.0,
         gradient_end:                 0.0,
         gradient_texture: None,
         gradient_points:  vec![],
         gradient_length:              Arc::new(AtomicCell::new(3000.0)),
         gradient_offset:              Arc::new(AtomicCell::new(100.0)),
         gradient_delta:               Arc::new(AtomicCell::new(10.0)),
         gradient_flat:                Arc::new(AtomicCell::new(0.2)),
         gradient_extreme:             Arc::new(AtomicCell::new(16.0)),
         vertical_scale:        Arc::new(AtomicCell::new(10.0)),
         gradient_distance: 0.0,
         gradient_pixmap: None,
         gradient_pixmap_width: 0,
         gradient_pixmap_height: 0,
         is_simulating: Arc::new(AtomicBool::new(false)),
         is_running: Arc::new(AtomicBool::new(false)),
         rider_data: Arc::new(AtomicCell::new(RiderData::default())),
         show_settings_dialog: false,
         show_settings_dialog_err: false,
         settings_dialog_message: String::new()
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

   #[allow(clippy::too_many_arguments)]
   pub(crate) fn update_distance_thread(ctx: Context, updated_distance: Arc<AtomicCell<f64>>,  track: Arc<Vec<TrackPoint>>,
     requested_delta: Arc<AtomicCell<f64>>, gradient_delta: Arc<AtomicCell<f64>>, rider_data: Arc<AtomicCell<RiderData>>,
     total_distance: f64, mode:Arc<AtomicCell<ViewMode>>, is_running: Arc<AtomicBool> )
   //--------------------------------------------------------------------------------------------------------------------
   {
      let mut last_distance: f64 = 0.0;
      let mut last_gradient_distance: f64 = 0.0;
      let mut distance: f64 = 0.0;
      while distance < total_distance
      {
         if !is_running.load(Ordering::Relaxed)
         {
            std::thread::sleep(Duration::from_secs(1));
            continue;
         }
         let mut rider = match super::frame::read_rider_data(3, Duration::from_millis(300))
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
            if (distance - last_distance) >= requested_delta.load()
            {
               updated_distance.store(distance);
               last_distance = distance;
               last_gradient_distance = distance;
               if let (Some(position), _) = find_closest_point(&track, distance)
               {
                  rider.latitude = position.point.lat;
                  rider.longitude = position.point.lon;
                  rider.altitude = position.altitude;
                  rider.distance = distance.round() as i32;
               }
               let rider_copy = RiderData::from(rider);
               rider_data.store(rider_copy);
               ctx.request_repaint();
               println!("Sent distance: {:.2} meters ({:.2}km)", distance, distance / 1000.0);
            } else if mode.load() == ViewMode::Gradient && (distance - last_gradient_distance) >= gradient_delta.load()
            {
               updated_distance.store(distance);
               last_gradient_distance = distance;
               if let (Some(position), _) = find_closest_point(&track, distance)
               {
                  rider.latitude = position.point.lat;
                  rider.longitude = position.point.lon;
                  rider.altitude = position.altitude;
                  rider.distance = distance.round() as i32;
               }
               let rider_copy = RiderData::from(rider);
               rider_data.store(rider_copy);
               ctx.request_repaint();
               // println!("Sent gradient distance: {:.2} meters ({:.2}km)", distance, distance / 1000.0);
            }
         }

         // if !is_running.load(Ordering::Relaxed) { break; }
         std::thread::sleep(Duration::from_secs(1));
      }
   }

   /// Simulates movement along a GPX track at 45km/h
   #[allow(clippy::too_many_arguments)]
   pub(crate) fn simulate_movement_thread( ctx: Context, updated_distance: Arc<AtomicCell<f64>>, track: Arc<Vec<TrackPoint>>,
      requested_delta: Arc<AtomicCell<f64>>, gradient_delta: Arc<AtomicCell<f64>>,
      simulated_speed: Arc<AtomicCell<f64>>, rider_data: Arc<AtomicCell<RiderData>>,
      total_distance: f64, mode:Arc<AtomicCell<ViewMode>>,
      is_sim_running: Arc<AtomicBool>, is_running: Arc<AtomicBool> )
   //-------------------------------------------------------------------------------------------------
   {
      let mut distance: f64 = 0.0;
      let mut last_gradient_distance: f64 = 0.0;
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
            // println!("Simulated distance: {:.2} meters ({:.2}km)", distance, distance / 1000.0);
         } else if mode.load() == ViewMode::Gradient && (distance - last_gradient_distance) >= gradient_delta.load()
         {
            updated_distance.store(distance);
            last_gradient_distance = distance;
            let mut rider = RiderData { distance: distance as i32, ..Default::default() };
            if let (Some(position), _) = find_closest_point(&track, distance)
            {
               rider.latitude = position.point.lat;
               rider.longitude = position.point.lon;
               rider.altitude = position.altitude;
            }
            rider.wind_speed = 10;
            rider.wind_angle = 60;
            rider_data.store(rider);
            last_gradient_distance = distance;
            ctx.request_repaint();
            println!("Sent gradient distance: {:.2} meters ({:.2}km)", distance, distance / 1000.0);
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

   pub(crate) fn check_broadcast_file(&mut self) -> (bool, bool)
   //----------------------------------
   {
      let broadcast_file = super::frame::get_broadcast_file();
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
               chrono::Duration::zero()
            }
         };
      }
      let is_aged = age.num_minutes() > 1;
      (is_exists, is_aged)
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

pub fn get_broadcast_directory_or_default() -> PathBuf
//---------------------------------------------
{
   if cfg!(target_os = "macos")
   {  // ~/TPVirtual/Broadcast/focus.json
      match dirs::home_dir()
      {
         | Some(dir) =>
         {
            dir.join("TPVirtual").join("Broadcast").clone()
         },
         | None => PathBuf::new()

      }
   }
   else
   {
      match dirs::document_dir()
      {
         | Some(dir) =>
         {
            dir.join("TPVirtual").join("Broadcast").clone()
         },
         | None => PathBuf::new()
      }
   }
}
