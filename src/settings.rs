//#![feature(os_str_display)]
use std::fs::File;
use std::io::Write;
use std::env;
use std::path::PathBuf;

use crate::ut;

const PROGRAM: &str = "GPXAssist";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Settings
{
   // #[serde(skip)] program: String,
   last_directory: PathBuf,
   pub(crate) gradient_length: f64,
   pub(crate) gradient_position: f64,
   pub(crate) flat_gradient_percentage: f64,
   pub(crate) extreme_gradient_percentage: f64,
   streetview_api_key: String,
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
         gradient_length: 3000.0,
         gradient_position: 500.0,
         flat_gradient_percentage: 0.5,
         extreme_gradient_percentage: 16.0,
         streetview_api_key: String::new(),
      }
   }
}

impl Settings
//===========
{
   pub fn new() -> Self
   {
      Settings::default()
   }

   pub fn get_settings(&self) -> Result<Settings, String>
   //-------------------------------------------
   {
      let _settings_dir = match self.get_settings_path()
      {
         Ok(pb) => pb,
         Err(e) => 
         {
            let errmsg = format!("Error getting settings path: {}", e);
            eprintln!("{errmsg}");
            return Err(errmsg);
         }
      };
      let mut settings_path = match self.get_settings_path()
      {
         Ok(p) => p,
         Err(_e) =>
         {
            match self.write_default_settings()
            {
               Ok(pp) => pp,
               Err(e) =>
               {
                  let errmsg = format!("Error creating default settings: {}", e);
                  return Err(errmsg);
               }
            }
         }
      };
      if ! settings_path.exists()
      {
         settings_path = match self.write_default_settings()
         {
            Ok(pp) => pp,
            Err(e) =>
            {
               eprintln!("Error creating default settings: {}", e);
               PathBuf::new()
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
      let mut config_file = self.get_config_path()?;
      config_file.push("settings.json");
      let mut file = File::create(&config_file)?;
      let json = serde_json::to_string(&self)?;
      file.write_all(json.as_bytes())?;
      // let file = File::create(&config_file)?;
      // let mut writer = BufWriter::new(file);
      // serde_json::to_writer(&mut writer, &settings)?;
      println!("Wrote settings {} to {}", json, config_file.display());
      Ok(config_file)
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


   /// Get OS specific path to the config directory for the program
   pub fn get_config_path(&self) -> Result<PathBuf, std::io::Error>
   //-----------------------------------------------------------------------------------------
   {
      match dirs::config_dir()
      {
         Some(p) =>
         {
            let pp = p.join(PROGRAM);
            if ! pp.exists()
            {
               match std::fs::create_dir_all(pp.as_path())
               {
                  Ok(_) => (),
                  Err(e) =>
                  {
                     return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to create config directory {}: {}", pp.display(), e)));
                  }
               }
            }
            Ok(pp)
         },
         None =>
         {
            let mut config_path = Settings::get_home_dir();

            if env::consts::OS == "windows"
            {
               config_path.push("Application Data/Local Settings/");
            }
            else if env::consts::OS == "macos" // No config dir ?
            {
               //config_path.push("Library/Application Support/");
            }
            else
            {
               config_path.push(".config/");
            }
            config_path.push(PROGRAM);
            if config_path.exists() && ! config_path.is_dir()
            {
               return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Config path {} exists and is not a directory", config_path.display())));
            }
            if !config_path.exists()
            {
               std::fs::create_dir_all(config_path.as_path())?;
            }
            Ok(config_path)
         }
      }
   }


   /// Get the path to the settings file for the program.
   pub fn get_settings_path(&self) -> Result<PathBuf, std::io::Error>
   //-------------------------------------------------------------------
   {
      let mut config_path = match self.get_config_path()
      {
         Ok(p) => p,
         Err(e) =>
         {
            eprintln!("Error getting settings path: {}", e);
            return Err(e);
         }
      };
      config_path.push("settings.json");
      Ok(config_path)
   }

   fn write_default_settings(&self) -> Result<PathBuf, std::io::Error>
   //-----------------------------------------------------------------------
   {
      let settings = Settings::default();
      let mut config_file = self.get_config_path()?;
      config_file.push("settings.json");
      let mut file = File::create(&config_file)?;
      let json = serde_json::to_string(&settings)?;
      file.write_all(json.as_bytes())?;
      // let file = File::create(&config_file)?;
      // let mut writer = BufWriter::new(file);
      // serde_json::to_writer(&mut writer, &settings)?;
      Ok(config_file)
   }


   fn read_settings(&self) -> Settings
   //-----------------------------------------------------------------
   {
      let mut config_file = match self.get_config_path()
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
