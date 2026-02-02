//#![feature(os_str_display)]
use std::{fs::File};
use std::io::Write;
use std::env;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Context, Vec2};

use crate::ui::get_broadcast_directory_or_default;
use crate::{ ui::{self, GPXAssistUI}, ut };

const PROGRAM: &str = "GPXAssist";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Settings
{
   // #[serde(skip)] program: String,
   #[serde(default = "Settings::get_home_dir")]
   last_directory: PathBuf,
   #[serde(default = "ui::get_broadcast_directory_or_default")]
   pub(crate) broadcast_directory: PathBuf,
   #[serde(default = "Settings::default_gradient_length")]
   pub(crate) gradient_length: f64,
   #[serde(default = "Settings::default_gradient_offset")]
   pub(crate) gradient_offset: f64,
   #[serde(default = "Settings::default_gradient_delta")]
   pub(crate) gradient_delta: f64,
   #[serde(default = "Settings::default_flat_gradient_percentage")]
   pub(crate) flat_gradient_percentage: f64,
   #[serde(default = "Settings::default_medium_gradient_percentage")]
   pub(crate) medium_gradient_percentage: f64,
   #[serde(default = "Settings::default_extreme_gradient_percentage")]
   pub(crate) extreme_gradient_percentage: f64,
   #[serde(default = "Settings::default_vertical_exaggeration")]
   pub(crate) vertical_exaggeration: f64,
   #[serde(default)]
   streetview_api_key: String,

   #[serde(skip)] show_api_key:              bool,
   #[serde(skip)] temp_api_key:              String,
   #[serde(skip)] temp_broadcast_dir:        PathBuf,
   #[serde(skip)] temp_gradient_length:      f64,
   #[serde(skip)] temp_gradient_offset:      f64,
   #[serde(skip)] temp_flat_gradient:        f64,
   #[serde(skip)] temp_medium_gradient:      f64,
   #[serde(skip)] temp_extreme_gradient:     f64,
   #[serde(skip)] temp_vertical_exaggeration: f64,
   #[serde(skip)] temp_gradient_delta: f64
}

impl Default for Settings
{
   fn default() -> Self
   //------------------
   {
      let default_open_dir = match dirs::home_dir()
      {
         Some(h) => h,
         None => env::temp_dir(),
      };
      Self
      {
         last_directory: default_open_dir,
         broadcast_directory: ui::get_broadcast_directory_or_default(),
         gradient_length: 3000.0,
         gradient_offset: 500.0,
         gradient_delta: 10.0,
         flat_gradient_percentage: 1.0,
         medium_gradient_percentage: 8.0,
         extreme_gradient_percentage: 16.0,
         vertical_exaggeration: 10.0,
         streetview_api_key: String::new(),

         show_api_key: false,
         temp_api_key: String::new(),
         temp_broadcast_dir: PathBuf::new(),
         temp_gradient_length: 3000.0,
         temp_gradient_offset: 500.0,
         temp_gradient_delta: 10.0,
         temp_flat_gradient: 0.5,
         temp_medium_gradient: 8.0,
         temp_extreme_gradient: 16.0,
         temp_vertical_exaggeration: 10.0
      }
   }
}

impl Settings
//===========
{
   fn default_gradient_length() -> f64
   {
      3000.0
   }

   fn default_gradient_offset() -> f64
   {
      500.0
   }

   fn default_gradient_delta() -> f64
   {
      10.0
   }

   fn default_flat_gradient_percentage() -> f64
   {
      1.0
   }

   fn default_medium_gradient_percentage() -> f64
   {
      8.0
   }

   fn default_extreme_gradient_percentage() -> f64
   {
      16.0
   }

   fn default_vertical_exaggeration() -> f64
   {
      10.0
   }

   pub fn new() -> Self
   {
      Settings::default()
   }

   pub fn get_settings(&self) -> Result<Settings, String>
//-------------------------------------------
   {
      let settings_path = match Settings::get_settings_path()
      {
         | Ok(p) => p,
         | Err(_e) => match Settings::write_default_settings()
         {
            | Ok(pp) => pp,
            | Err(e) =>
            {
               let errmsg = format!("Error on get settings: {}", e);
               return Err(errmsg);
            }
         },
      };

      if !settings_path.exists()
      {
         match Settings::write_default_settings()
         {
            | Ok(_) => (),
            | Err(e) =>
            {
               eprintln!("Error creating default settings: {}", e);
               // PathBuf::new()
            }
         };
      }
      Ok(self.read_settings())
   }


   pub fn get_settings_or_default(&self) -> Settings
   //-------------------------------------------
   {
      match self.get_settings()
      {
         Ok(s) => s,
         Err(_) => Settings::default(),
      }
   }

   pub(crate) fn write_settings(&self) -> Result<PathBuf, std::io::Error>
   //-----------------------------------------------------------------------
   {
      let settings_path = match Settings::get_settings_path()
      {
         | Ok(p) => p,
         | Err(_) => match Settings::write_default_settings()
         {
            | Ok(pp) => pp,
            | Err(e) =>
            {
               // println!("Error on get settings: {}", e);
               return Err(e);
            }
         },
      };
      let mut file = File::create(&settings_path)?;
      for retry in 0..3
      {
         match file.try_lock()
         {
            | Ok(_) => break,
            | Err(e) =>
            {
               if retry == 2
               {
                  let errmsg = format!("Failed to lock settings file {}: {}", settings_path.display(), e);
                  // println!("{errmsg}");
                  return Err(std::io::Error::other(errmsg)); 
               }
               std::thread::sleep(std::time::Duration::from_millis(500));
            }
         }
      }
      let json = serde_json::to_string_pretty(&self)?;
      file.write_all(json.as_bytes())?;
      // println!("Wrote settings {} to {}", json, settings_path.display());
      Ok(settings_path)
   }

   pub fn get_streetview_api_key(&self) -> Result<String, String>
   //--------------------------------------
   {
      let encrypted_bytes = match hex::decode(&self.streetview_api_key)
      {
         Ok(bytes) => bytes,
         Err(e) =>
         {
            let errmsg = format!("Failed to hex decode encrypted password: {}", e);
            // self.toast_manager.error(errmsg);
            return Err(errmsg);
         }
      };
      if encrypted_bytes.is_empty()
      {
         return Err("Street View API key is not set.".to_string());
      }
      {
         match ut::decrypt(&encrypted_bytes)
         {
            | Ok(decrypted_key) =>
            {
               Ok(decrypted_key)
            }
            | Err(e) =>
            {
               let errmsg = format!("Failed to decrypt Street View API key: {}", e);
               eprintln!("{errmsg}");
               // self.toast_manager.error(errmsg);
               Err(errmsg)
            }
         }
      }
   }

   fn set_streetview_api_key_from_tmp(&mut self) -> Result<(), String>
   //----------------------------------------------------------------
   {
      match ut::encrypt(&self.temp_api_key)
      {
         | Ok(encrypted_data) =>
         {
            self.streetview_api_key = hex::encode(encrypted_data);
            match self.write_settings()
            {
               | Ok(_) => (),
               | Err(e) =>
               {
                  let errmsg = format!("Failed to write settings file: {}", e);
                  eprintln!("{errmsg}");
                  return Err(errmsg);
               }
            }
            Ok(())
         }
         | Err(e) =>
         {
            let errmsg = format!("Failed to encrypt Street View API key: {}", e);
            eprintln!("{errmsg}");
            // self.toast_manager.error(errmsg);
            Err(errmsg)
         }
      }
   }

   pub fn set_streetview_api_key(&mut self, api_key: &str) -> Result<(), String>
   //-------------------------------------------------------
   {
      match ut::encrypt(api_key)
      {
         | Ok(encrypted_data) =>
         {
            self.streetview_api_key = hex::encode(encrypted_data);
            match self.write_settings()
            {
               | Ok(_) => (),
               | Err(e) =>
               {
                  let errmsg = format!("Failed to write settings file: {}", e);
                  eprintln!("{errmsg}");
                  return Err(errmsg);
               }
            }
            Ok(())
         }
         | Err(e) =>
         {
            let errmsg = format!("Failed to encrypt Street View API key: {}", e);
            eprintln!("{errmsg}");
            // self.toast_manager.error(errmsg);
            Err(errmsg)
         }
      }
   }

   #[allow(dead_code)]
   pub fn set_last_directory(&mut self, path: &str) -> bool
   //-------------------------------------------
   {
      let path = PathBuf::from(path);
      if path.is_dir()
      {
         self.last_directory = path;
         match self.write_settings()
         {
            | Ok(_) => (),
            | Err(e) =>
            {
               eprintln!("Failed to write settings file: {}", e);
               return false;
            }
         }
         return true;
      }
      eprintln!("{} is not a directory", path.display());
      false
   }

   pub fn set_last_directorybuf(&mut self, path: &PathBuf) -> bool
   //-------------------------------------------
   {
      if path.is_dir()
      {
         self.last_directory = path.clone();
         match self.write_settings()
         {
            | Ok(_) => (),
            | Err(e) =>
            {
               eprintln!("Failed to write settings file: {}", e);
               return false;
            }
         }
         return true;
      }
      eprintln!("{} is not a directory", path.display());
      false
   }

   #[allow(dead_code)]
   pub fn get_last_directory(&self) -> String
   //-------------------------------------------
   {
      self.last_directory.display().to_string()
   }

   pub fn get_last_directorybuf(&self) -> PathBuf
   //-------------------------------------------
   {
      self.last_directory.clone()
   }


   
   pub fn get_config_path() -> Result<PathBuf, std::io::Error>
   //-----------------------------------------------------------------------------------------
   {
      match dirs::config_dir() // cargo add dirs
      {
         | Some(p) =>
         {
            let pp = p.join(PROGRAM);
            if !pp.exists()
            {
               match std::fs::create_dir_all(pp.as_path())
               {
                  | Ok(_) => (),
                  | Err(e) =>
                  {
                     return Err(std::io::Error::other(format!("Failed to create config directory {}: {}",
                                                            pp.display(), e)));
                  }
               }
            }
            Ok(pp)
         }
         | None =>
         {
            let mut config_path = Settings::get_home_dir();

            if env::consts::OS == "windows"
            {
               let mut pp = config_path.clone();
               pp.push("AppData/Local");
               if pp.is_dir()
               {
                  config_path.push("AppData/Local");
               }
               else
               {
                  pp.pop();
                  pp.pop();
                  pp.push("Local Settings/");
                  if pp.is_dir()
                  {
                     config_path.push("Local Settings/");
                  }
                  else
                  {
                     config_path.push("Application Data/Local Settings/");
                  }
               }
            }
         else if env::consts::OS == "macos"            
            {
               config_path.push(Settings::get_home_dir());
               config_path.push(".config/");
               if ! config_path.is_dir()
               {
                  config_path.pop();
                  config_path.push("Library/");
                  config_path.push("Application Support/");
                  if ! config_path.is_dir()
                  {
                     config_path.pop();
                     config_path.pop();
                  }
               }
            }
            else
            {
               config_path.push(".config/");
            }
            config_path.push(PROGRAM);
            if config_path.exists() && !config_path.is_dir()
            {
               return Err(std::io::Error::other(format!("Config path {} exists and is not a directory",
                                                      config_path.display())));
            }
            if !config_path.exists()
            {
               std::fs::create_dir_all(config_path.as_path())?;
            }
            Ok(config_path)
         }
      }
   }

   pub fn get_settings_path() -> Result<PathBuf, std::io::Error>
   //-------------------------------------------------------------------
   {
      let mut config_path = match Settings::get_config_path()
      {
         | Ok(p) => p,
         | Err(e) =>
         {
            eprintln!("Error getting settings path: {}", e);
            return Err(e);
         }
      };
      config_path.push("settings.json");
      Ok(config_path)
   }


   pub fn write_default_settings() -> Result<PathBuf, std::io::Error>
//-----------------------------------------------------------------------
   {
      let settings = Settings::default();
      let mut config_file = Settings::get_config_path()?;
      config_file.push("settings.json");
      let mut file = File::create(&config_file)?;
      let json = serde_json::to_string_pretty(&settings)?;
      file.write_all(json.as_bytes())?;
      // let file = File::create(&config_file)?;
      // let mut writer = BufWriter::new(file);
      // serde_json::to_writer(&mut writer, &settings)?;
      Ok(config_file)
   }

   fn read_settings(&self) -> Settings
   //-----------------------------------------------------------------
   {
      let mut config_file = match Settings::get_config_path()
      {
         Ok(p) => p,
         Err(e) =>
         {
            eprintln!("Error getting settings path: {}", e);
            return Settings::default();
         }
      };
      config_file.push("settings.json");
      if !config_file.exists()
      {
         return Settings::default();
      }
      let file = match  File::open(&config_file)
      {
         Ok(f) => f,
         Err(e) =>
         {
            eprintln!("Error opening settings file: {}", e);
            return Settings::default();
         }
      };
      let settings: Settings = match serde_json::from_reader(file)
      {
         Ok(s) => s,
         Err(e) =>
         {
            eprintln!("Error reading settings: {}", e);
            Settings::default()
         }
      };
      settings.clone()
   }

   pub fn open_settings_dialog(&mut self, assist: &mut GPXAssistUI)
   //---------------------------------
   {
      // let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
      // let settings_lock = settings.lock();

      // if let Ok(api_key) = settings_lock.get_streetview_api_key()
      // {
      //    settings_lock.temp_api_key = api_key;
      // }
      // else
      // {
      //    settings_lock.temp_api_key.clear();
      // }
      self.temp_api_key = match self.get_streetview_api_key()
      {
         Ok(k) => k,
         Err(_) => String::new(),
      };

      self.temp_broadcast_dir = self.broadcast_directory.clone();
      self.temp_gradient_length = self.gradient_length;
      self.temp_gradient_offset = self.gradient_offset;
      self.temp_gradient_delta = self.gradient_delta;
      self.temp_flat_gradient = self.flat_gradient_percentage;
      self.temp_medium_gradient = self.medium_gradient_percentage;
      self.temp_extreme_gradient = self.extreme_gradient_percentage;
      self.temp_vertical_exaggeration = self.vertical_exaggeration;
      self.show_api_key = false;

      // Show the dialog
      assist.show_settings_dialog = true;
   }

   pub fn show_settings_dialog(&mut self, assist: &mut GPXAssistUI, ctx: &Context)
   //------------------------------------------------
   {
      if !assist.show_settings_dialog
      {
         return;
      }

      let mut status_message: String = String::default();
      let mut status_color = Color32::GREEN;
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
               .show(ui, |ui|
               {
                  ui.label("Street View API Key:");
                  ui.horizontal(|ui|
                  {
                     ui.add_sized(Vec2::new(400.0, 30.0),
                         egui::TextEdit::singleline(&mut self.temp_api_key)
                        .hint_text("Enter your Google API key")
                        .password(!self.show_api_key)
                        // .desired_width(300.0)
                     ).on_hover_text("Enter your Google API key");

                     // Toggle button to show/hide API key
                     let button_text = if self.show_api_key { "  🙈  " } else { "  👁  " };
                     if ui.button(button_text).clicked() {
                        self.show_api_key = !self.show_api_key;
                     }
                  });
                  ui.end_row();

                  let mut dir_color = Color32::GREEN;
                  let dir =
                  if self.temp_broadcast_dir.display().to_string().trim().is_empty()
                  {
                     dir_color = Color32::YELLOW;
                     status_color = Color32::YELLOW;
                     status_message = "WARN: Broadcast directory is not set.".to_string();
                     // self.temp_broadcast_dir.clone()
                     get_broadcast_directory_or_default()
                  }
                  else if ! self.temp_broadcast_dir.exists()
                  {
                     dir_color = Color32::RED;
                     status_color = Color32::RED;
                     status_message = format!("Directory {:?} does not exist.", self.temp_broadcast_dir);
                     self.temp_broadcast_dir.clone()
                     // get_broadcast_directory_or_default()
                  }
                  else
                  {
                     if ! self.temp_broadcast_dir.is_dir()
                     {
                        dir_color = Color32::RED;
                        status_color = Color32::RED;
                        status_message = format!("Directory {:?} is not a directory.", self.temp_broadcast_dir);
                        // PathBuf::new()
                        self.temp_broadcast_dir.clone()
                     }
                     else
                     {
                        let file_path = self.temp_broadcast_dir.join("focus.json");
                        if ! file_path.exists() || ! file_path.is_file()
                        {
                           dir_color = Color32::YELLOW;
                           status_color = Color32::YELLOW;
                           status_message = format!("WARN: Broadcast file {:?} not found.", file_path);
                           // PathBuf::new()
                        }
                        else
                        {
                           status_message = "".to_string();
                           // self.temp_broadcast_dir.clone()
                        }
                        self.temp_broadcast_dir.clone()
                     }
                  };
                  let mut dir_string = dir.display().to_string();

                  ui.label("Broadcast Dir:");
                  ui.horizontal(|ui|
                  {
                     let text_color = if dir_color == Color32::RED || dir_color == Color32::YELLOW
                     {
                        Color32::BLACK
                     }
                     else
                     {
                        Color32::DARK_BLUE
                     };
                     ui.style_mut().visuals.override_text_color = Some(text_color);
                     ui.add_sized( egui::Vec2::new(400.0, 30.0), egui::TextEdit::singleline(&mut dir_string).background_color(dir_color));
                     if ui.button("  📂  ").clicked()
                     {
                        // let dialog_future = rfd::AsyncFileDialog::new().set_directory(home).pick_file();
                        if let Some(selected_dir) = rfd::FileDialog::new().set_directory(&dir).pick_folder()
                        {
                           self.temp_broadcast_dir = selected_dir;
                        }
                     }
                  });
                  ui.end_row();

                  // if ! status_message.is_empty()
                  // {
                  //    ui.horizontal(|ui| { ui.label(egui::RichText::new(&status_message).color(dir_color).text_style(egui::TextStyle::Small)); });
                  //    ui.label("");
                  //    ui.end_row();
                  // }

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
                     egui::DragValue::new(&mut self.temp_gradient_offset)
                     .range(100.0..=2000.0)
                     .speed(10.0))
                     .on_hover_text("The position within the gradient section where the rider currently is positioned (metres)");
                  ui.end_row();

                  ui.label("Gradient Delta (m):");
                  ui.add_sized(
                     egui::Vec2::new(100.0, 30.0),
                     egui::DragValue::new(&mut self.temp_gradient_delta)
                     .range(1.0..=100.0)
                     .speed(10.0))
                     .on_hover_text(format!("The distance in metres to travel before redrawing the gradient display with rider positioned at {:.2} (metres)", self.temp_gradient_offset));
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

                  ui.label("Medium Gradient (%):");
                  ui.add_sized(
                     egui::Vec2::new(100.0, 30.0),
                     egui::DragValue::new(&mut self.temp_medium_gradient)
                     .range(2.0..=16.0)
                     .speed(0.5)
                     .max_decimals(1))
                     .on_hover_text("The gradient considered to be 'medium'.")
                     .on_hover_text("If flat to medium then gradient color is a shade of yellow; if medium to extreme then red.");
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

                  ui.label("Vertical Exaggeration:");
                  ui.add_sized(
                     egui::Vec2::new(100.0, 30.0),
                     egui::DragValue::new(&mut self.temp_vertical_exaggeration)
                     .range(1.0..=50.0)
                     .speed(0.5)
                     .max_decimals(1))
                     .on_hover_text("Vertical exaggeration factor for elevation plot (1.0 = true scale, 10.0 = default, higher = more vertical stretch)");
                  ui.end_row();
               });

            ui.separator();

            if ! status_message.is_empty()
            {
               ui.horizontal(|ui| { ui.label(egui::RichText::new(&status_message).color(status_color).text_style(egui::TextStyle::Small)); });
               ui.separator();
            }

            ui.horizontal(|ui| {
               if ui.button("Save").clicked()
               {
                  // let settings = SETTINGS.get_or_init(|| Arc::new(parking_lot::Mutex::new(Settings::new().get_settings_or_default())));
                  // let mut settings_lock = settings.lock();

                  // Save API key
                  if !self.temp_api_key.is_empty()
                  {
                     match self.set_streetview_api_key_from_tmp()
                     {
                        | Ok(_) =>
                        {
                           // toast_manager.success("Settings saved successfully", Some(Duration::from_secs(3)));
                           assist.encrypted_api_key = Some(self.temp_api_key.clone());
                           assist.settings_dialog_message = "Settings saved successfully".to_string();
                        }
                        | Err(e) =>
                        {
                           assist.settings_dialog_message = format!("Failed to save API key: {}", e);
                           //toast_manager.error(&format!("Failed to save API key: {}", e), None);
                        }
                     }
                  }

                  // Update gradient settings
                  self.gradient_length = self.temp_gradient_length;
                  self.gradient_offset = self.temp_gradient_offset;
                  self.gradient_delta = self.temp_gradient_delta;
                  self.flat_gradient_percentage = self.temp_flat_gradient;
                  self.medium_gradient_percentage = self.temp_extreme_gradient;
                  self.extreme_gradient_percentage = self.temp_extreme_gradient;
                  self.vertical_exaggeration = self.temp_vertical_exaggeration;

                  // Write settings to file
                  match self.write_settings()
                  {
                     | Ok(_) =>
                     {
                        assist.show_settings_dialog_err = false;
                     },
                     | Err(e) =>
                     {
                        assist.settings_dialog_message = format!("Failed to write settings: {}", e);
                        assist.show_settings_dialog_err = true;
                        // toast_manager.error(&format!("Failed to write settings: {}", e), None);
                     }
                  }

                  // Close dialog
                  assist.show_settings_dialog = false;
               }

               if ui.button("Cancel").clicked()
               {
                  // Reset temp values
                  self.temp_api_key.clear();
                  self.temp_gradient_length = 3000.0;
                  self.temp_gradient_offset = 500.0;
                  self.temp_gradient_delta = 10.0;
                  self.temp_flat_gradient = 0.5;
                  self.temp_medium_gradient = 8.0;
                  self.temp_extreme_gradient = 16.0;
                  self.temp_vertical_exaggeration = 10.0;
                  self.show_api_key = false;

                  // Close dialog
                  assist.show_settings_dialog = false;
                  assist.show_settings_dialog_err = false;
                  assist.settings_dialog_message = "".to_string();
               }
            });
         });
   }

   fn get_home_fallbacks() -> PathBuf
   //--------------------------------
   {
      if cfg!(target_os = "linux")
      {
         return PathBuf::from("~/")
      }
      else if cfg!(target_os = "windows")
      {
         return PathBuf::from("C:/Users/Public")
      }
      return PathBuf::from("~/")
   }

   pub fn get_home_dir() -> PathBuf
   //-------------------------------
   {
      match dirs::home_dir()
      {
         Some(h) => h,
         None => Settings::get_home_fallbacks()
      }
   }

   #[allow(dead_code)]
   pub fn get_home_dir_string() -> String
   //-------------------------------
   {
      match dirs::home_dir()
      {
         Some(h) => h.display().to_string(),
         None =>
         {
            let pp = Settings::get_home_fallbacks();
            pp.display().to_string()
         }
      }
   }
}

unsafe impl Sync for Settings {}
