use std::{cell::RefCell, fs, sync::OnceLock};
use std::sync::Arc;
// use std::rc::Rc;

use clap::Parser;
use eframe::{CreationContext, egui};
use lazy_static::lazy_static;


mod settings;
mod components;
mod gpx;
pub mod ui;
mod ut;
pub mod data;

use crate::{gpx::TrackPoint, ui::GPXAssistUI};
use settings::Settings;


#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args
{
   /// Select gpx distance calculation method h = Haversine, e = ECEF
   #[arg(short = 'm', long = "method", default_value = "e")]
   method: char,

   #[arg(short = 'p', long = "password", default_value = "", help = "Encrypt new password and write to config file")]
   password: String,

   /// Optional GPX file path
   #[arg()]
   file_path: Option<String>,
}

struct StartupParameters
{
   method:    Option<gpx::DistanceMethod>,
   file_path: Option<String>,
}

static STARTUP_PARAMS: parking_lot::Mutex<RefCell<Option<StartupParameters>>> = parking_lot::Mutex::new(RefCell::new(None));
static SETTINGS: OnceLock<Arc<parking_lot::Mutex<Settings>>> = OnceLock::new();

lazy_static!
{
//   pub(crate) static ref SETTINGS: Arc<parking_lot::Mutex<Settings>> = Arc::new(parking_lot::Mutex::new(Settings::new().get_settings("GPXAssist")));
   // pub(crate) static ref SETTINGS: Rc<RefCell<Settings>> = Rc::new(RefCell::new(Settings::new().get_settings()));
}

fn main()
{
   env_logger::init();
   {
      let cmdline_opts = STARTUP_PARAMS.lock();
      let args = Args::parse();
      let method = match args.method
      {
         | 'h' | 'H' => gpx::DistanceMethod::Haversine,
         | 'e' | 'E' => gpx::DistanceMethod::ECEF,
         | _ =>
         {
            eprintln!("Invalid method. Use 'h' for Haversine or 'e' for ECEF.");
            return;
         }
      };

      let update_password = args.password.trim();

      let mut file_path: Option<String> = None;
      if let Some(filepath) = args.file_path
      {
         let gpx_file_path = std::path::Path::new(&filepath);
         let metadata = match fs::metadata(gpx_file_path)
         {
            | Ok(meta) => meta,
            | Err(_) =>
            {
               eprintln!("The path {filepath} is not a valid file.");
               return;
            }
         };
         if !metadata.is_file()
         {
            eprintln!("The path {filepath} is not a valid file.");
            return;
         }
         file_path = Some(filepath.clone());
      }
      if !update_password.is_empty()
      {         
         let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
         match settings.lock().set_streetview_api_key(&update_password)
         {
            | Ok(_) =>
            {               
               println!("Password encrypted and saved to settings file");
               return
            },
            | Err(e) =>
            {
               eprintln!("Error saving settings with new password: {}", e);
               return
            }
         }          
      }

      cmdline_opts.replace(Some(StartupParameters { method:    Some(method.clone()),
                                                    file_path: file_path, }));
   }
   let options = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 1024.0]),
                                         ..Default::default() };
   let ret = eframe::run_native("GPXAssist",
                                options,
                                Box::new(|cc| {
                                   egui_extras::install_image_loaders(&cc.egui_ctx);
                                   Ok(Box::new(GPXAssistUI::new(cc)))
                                }));
   if let Err(e) = ret
   {
      eprintln!("Error starting user interface: {e}");
   }
}
