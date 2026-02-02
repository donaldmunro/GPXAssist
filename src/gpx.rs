#![allow(non_snake_case)]
use std::{cmp::Ordering, f64, fs::{self, File}, io::BufReader, path::Path};

use gpx::{Gpx, read};
use fitparser::de::{DecodeOption, from_reader_with_options};
use fitparser::profile::MesgNum;

use crate::data::{self, INVALID_COORDINATE};

// const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

const WGS84_A: f64 = 6378137.0; // Semi-major axis
const WGS84_F: f64 = 1.0 / 298.257223563; // Flattening
const WGS84_E_SQ: f64 = WGS84_F * (2.0 - WGS84_F); // Eccentricity squared

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct WGS84Position
{
   pub latitude: f64,
   pub longitude: f64,
   pub altitude: f64
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint
{
   pub distance: f64, // Cumulative distance in meters
   pub point:    WGS84Position,
   pub heading:  f64, // Bearing/heading in degrees (0-360)
}

impl Default for TrackPoint
{
   fn default() -> Self
   {
      TrackPoint
      {
         distance: -1.0,
         point:    WGS84Position { latitude: 0.0, longitude: 0.0, altitude: 0.0 },
         heading:  0.0,
      }
   }
}

impl From<WGS84Position> for TrackPoint
{
   fn from(pos: WGS84Position) -> Self
   {
      TrackPoint
      {
         distance: 0.0,
         point:    pos,
         heading:  0.0,
      }
   }
}

impl From<&WGS84Position> for TrackPoint
{
   fn from(pos: &WGS84Position) -> Self
   {
      TrackPoint
      {
         distance: 0.0,
         point:    *pos,
         heading:  0.0,
      }
   }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ECEFCoord
{
   x: f64,
   y: f64,
   z: f64,
}

// fn haversine_distance(p1: Point, p2: Point) -> f64
// //------------------------------------------------
// {
//    let lat1_rad = p1.lat.to_radians();
//    let lon1_rad = p1.lon.to_radians();
//    let lat2_rad = p2.lat.to_radians();
//    let lon2_rad = p2.lon.to_radians();

//    let d_lat = lat2_rad - lat1_rad;
//    let d_lon = lon2_rad - lon1_rad;

//    let a = (d_lat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (d_lon / 2.0).sin().powi(2);
//    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

//    EARTH_RADIUS_METERS * c
// }

fn geodetic_to_ecef(p: WGS84Position) -> ECEFCoord
//----------------------------------------
{
   let lat_rad = p.latitude.to_radians();
   let lon_rad = p.longitude.to_radians();

   // Prime vertical radius of curvature
   let n = WGS84_A / (1.0 - WGS84_E_SQ * lat_rad.sin().powi(2)).sqrt();

   // h (height) is assumed to be 0
   let x = n * lat_rad.cos() * lon_rad.cos();
   let y = n * lat_rad.cos() * lon_rad.sin();
   let z = (n * (1.0 - WGS84_E_SQ)) * lat_rad.sin();

   ECEFCoord { x, y, z }
}

/// Calculates distance by converting to ECEF coordinates (ellipsoidal Earth).
fn ECEF_distance(p1: WGS84Position, p2: WGS84Position) -> f64
//-------------------------------------------
{
   let ecef1 = geodetic_to_ecef(p1);
   let ecef2 = geodetic_to_ecef(p2);

   // Simple Euclidean distance between the two 3D points
   ((ecef2.x - ecef1.x).powi(2) + (ecef2.y - ecef1.y).powi(2) + (ecef2.z - ecef1.z).powi(2)).sqrt()
}

pub fn build_track_data_gpx(path: &Path) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>>
//-------------------------------------------------------------------------------------------------------------
{
   let file = File::open(path)?;
   let reader = BufReader::new(file);
   let gpx: Gpx = read(reader)?;

   let track_segment = gpx.tracks.first()
                          .and_then(|track| track.segments.first())
                          .ok_or("GPX file does not contain a track segment.")?;

   let mut track_data = Vec::new();
   let mut cumulative_distance = 0.0;
   let mut last_point: Option<WGS84Position> = None;

   for point in &track_segment.points
   {
      let current_altitude = point.elevation.unwrap_or(0.0);
      let current_point = WGS84Position { latitude: point.point().y(), longitude: point.point().x(), altitude: current_altitude };
      let mut current_heading = 0.0;

      if let Some(prev_point) = last_point
      {
         let segment_distance = ECEF_distance(prev_point, current_point);
         cumulative_distance += segment_distance;
         current_heading = calculate_bearing(prev_point.latitude, prev_point.longitude, current_point.latitude, current_point.longitude);
      }

      track_data.push(TrackPoint {  distance: cumulative_distance,
                                    point:    current_point,
                                    heading:  current_heading,
                                 });
      // println!("GPX point: {:?}", TrackPoint {  distance: cumulative_distance,
      //                               point:    current_point,
      //                               heading:  current_heading,
      //                            });

      last_point = Some(current_point);
   }

   Ok(track_data)
}

pub fn process_gpx(file_path: &str) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>>
//-------------------------------------------------------
{
   let gpx_file_path = std::path::Path::new(file_path);
   let metadata = match fs::metadata(gpx_file_path)
   {
      | Ok(meta) => meta,
      | Err(e) =>
      {
         return Err(Box::new(e));
      }
   };
   if !metadata.is_file()
   {
      eprintln!("The path {} is not a valid file.", file_path);
      return Err(format!("Not a file {}.", file_path).into());
   }
   let track = match build_track_data_gpx(gpx_file_path)
   {
      | Ok(data) =>
      {
         println!("Successfully processed {} points.", data.len());
         let total_dist = data.last().map_or(0.0, |p| p.distance);
         println!("Total track distance: {:.2} meters.", total_dist);
         data
      }
      | Err(e) =>
      {
         let msg = format!("Error processing gpx file {}: {}", file_path, e);
         return Err(msg.into());
      }
   };
   Ok(track)
}

pub fn build_track_data_fit(path: &Path) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>>
//-------------------------------------------------------------------------------------------------------------
{
   let mut file = File::open(path)?;
   let opts = [DecodeOption::SkipHeaderCrcValidation,
               DecodeOption::SkipDataCrcValidation].iter().copied().collect();
   let mut track_data = Vec::new();
   let mut last_point: Option<WGS84Position> = None;
   let mut errors = 0;
   let mut latitude: f64;
   let mut longitude: f64;
   let mut latitude_int: i64;
   let mut longitude_int: i64;
   let mut altitude: f64;
   let mut enhanced_altitude: f64;
   let mut distance: f64;
   // let mut last_distance : f64 = f64::NAN;
   let mut last_latitude_int = INVALID_COORDINATE;
   let mut last_longitude_int = INVALID_COORDINATE;
   let mut cumulative_distance: f64 = 0.0;
   for data in from_reader_with_options(&mut file, &opts)?
   {
      if errors > 100
      {
         return Err("Too many errors parsing FIT file data.".into());
      }
      if data.kind() == MesgNum::Record
      {
         let fields = data.fields();
         latitude_int = INVALID_COORDINATE;
         longitude_int = INVALID_COORDINATE;
         latitude = f64::NAN;
         longitude = f64::NAN;
         altitude = f64::NAN;
         enhanced_altitude = f64::NAN;
         distance = f64::NAN;
         for field in fields
         {
            if field.name() == "position_lat" || field.name() == "position_long"
            {
               let value = match field.value()
               {
                  fitparser::Value::SInt32(v) =>
                  {
                     Some(*v)
                  },
                  _ => None,
               };
               if let Some(degrees) = value
               {
                  // println!("{}: {}", field.name(), degrees);
                  match field.name()
                  {
                     "position_lat"  =>
                     {
                        latitude_int = degrees as i64;
                        latitude = data::RiderDataJSON::semicircles_2_degrees(latitude_int)
                     }
                     "position_long" =>
                     {
                        longitude_int = degrees as i64;
                        longitude = data::RiderDataJSON::semicircles_2_degrees(longitude_int);
                     }
                     _ => {},
                  }
               }
            }
            if  field.name() == "distance" || field.name() == "altitude" || field.name() == "enhanced_altitude"
            {
               let value = match field.value()
               {
                  fitparser::Value::Float64(v) =>
                  {
                     *v
                  },
                  _ => f64::NAN,
               };
               match field.name()
               {
                  "distance"           => distance = value, // metres not km
                  "altitude"           => altitude = value,
                  "enhanced_altitude"  => enhanced_altitude = value,
                  _                    => {},
               }
            }
         }
         if !latitude.is_nan() && !longitude.is_nan() && (!enhanced_altitude.is_nan() || !altitude.is_nan())
         {
            if ! enhanced_altitude.is_nan() && enhanced_altitude > -50.0 && enhanced_altitude < 100000.0
            {
               altitude = enhanced_altitude;
            }
            if altitude.is_nan()
            {
               errors += 1;
               continue;
               // return Err("Both altitude and enhanced_altitude are NaN.".into());
            }
            if //latitude_int != INVALID_COORDINATE && longitude_int != INVALID_COORDINATE &&
               //last_latitude_int != INVALID_COORDINATE && last_longitude_int != INVALID_COORDINATE &&
               last_latitude_int == latitude_int && last_longitude_int == longitude_int
            {
               // println!("NoChange: Latitude: {} == {}, Longitude: {} == {}", latitude_int, last_latitude_int, longitude_int, last_longitude_int);
               continue;
            }
            let point = WGS84Position { latitude: latitude, longitude: longitude, altitude: altitude };
            if distance.is_nan() && let Some(prev_point) = last_point
            {
               let segment_distance = ECEF_distance(prev_point, point);
               cumulative_distance += segment_distance;
               distance = cumulative_distance;
            }
            else
            {
               if ! distance.is_nan()
               {
                  cumulative_distance = distance;
               }
            }
            if distance.is_nan()
            {
               errors += 1;
               continue;
            }
            // if ! last_distance.is_nan() && distance <= last_distance
            // {
            //    continue;
            // }
            let mut heading: f64 = 0.0;
            if let Some(prev_point) = last_point
            {
               heading = calculate_bearing(prev_point.latitude, prev_point.longitude, point.latitude, point.longitude);
            }
            track_data.push(TrackPoint {  distance: distance,
                                          point:    point,
                                          heading:  heading,
                                       });
            println!("FIT point: {:?}", TrackPoint {  distance: distance,
                                          point:    point,
                                          heading:  heading,
                                       });
            last_latitude_int = latitude_int;
            last_longitude_int = longitude_int;
            // last_distance = distance;
            last_point = Some(point);
         }
         else
         {
            errors += 1;
         }
      }
   }
   Ok(track_data)
}

pub fn process_fit(file_path: &str) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>>
//-------------------------------------------------------
{
   let fit_file_path = std::path::Path::new(file_path);
   let metadata = match fs::metadata(fit_file_path)
   {
      | Ok(meta) => meta,
      | Err(e) =>
      {
         return Err(Box::new(e));
      }
   };
   if !metadata.is_file()
   {
      eprintln!("The path {} is not a valid file.", file_path);
      return Err(format!("Not a file {}.", file_path).into());
   }
   let track = match build_track_data_fit(fit_file_path)
   {
      | Ok(data) =>
      {
         println!("Successfully processed {} points.", data.len());
         let total_dist = data.last().map_or(0.0, |p| p.distance);
         println!("Total track distance: {:.2} meters.", total_dist);
         data
      }
      | Err(e) =>
      {
         let msg = format!("Error processing gpx file {}: {}", file_path, e);
         return Err(msg.into());
      }
   };
   Ok(track)

}

/// Finds the closest TrackPoint in the dataset to a target distance using binary search.
/// Returns the matching point (if any) along with its index, or -1 when the track is empty.
pub fn find_closest_point(track_data: &[TrackPoint], target_distance: f64) -> (Option<TrackPoint>, i64)
//--------------------------------------------------------------------------------------------------------
{
   if track_data.is_empty()
   {
      return (None, -1);
   }

   let search_result = track_data.binary_search_by(|probe|
      probe.distance.partial_cmp(&target_distance).unwrap_or(Ordering::Equal));

   match search_result
   {
      | Ok(index) => (Some(track_data[index]), index as i64),
      | Err(index) =>
      {
         let chosen_index = if index == 0
         {
            0
         }
         else if index >= track_data.len()
         {
            track_data.len() - 1
         }
         else
         {
            let prev = track_data[index - 1];
            let next = track_data[index];
            if (target_distance - prev.distance) <= (next.distance - target_distance) { index - 1 } else { index }
         };

         (Some(track_data[chosen_index]), chosen_index as i64)
      }
   }
}

pub fn calculate_bearing(from_latitude: f64, from_longitude: f64, to_latitude: f64, to_longitude: f64) -> f64
//-------------------------------------------------------------
{
   if from_latitude == INVALID_COORDINATE as f64 || from_longitude == INVALID_COORDINATE  as f64||
      to_latitude == INVALID_COORDINATE as f64 || to_longitude == INVALID_COORDINATE as f64
   {
      return 0.0;
   }
   // Convert from degrees to radians
   let from_lat_rad = from_latitude.to_radians();
   let from_lon_rad = from_longitude.to_radians();
   let to_lat_rad = to_latitude.to_radians();
   let to_lon_rad = to_longitude.to_radians();

   let delta_lon = to_lon_rad - from_lon_rad;

   let y = delta_lon.sin() * to_lat_rad.cos();
   let x = from_lat_rad.cos() * to_lat_rad.sin() - from_lat_rad.sin() * to_lat_rad.cos() * delta_lon.cos();

   let bearing_rad = y.atan2(x);

   // Convert from radians to degrees and normalize to 0-360 range
   let bearing_deg = bearing_rad.to_degrees();
   (bearing_deg + 360.0) % 360.0
}
