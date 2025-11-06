#![allow(non_snake_case)]
use std::{cmp::Ordering,
          fs::{self, File},
          io::BufReader,
          path::Path};

use gpx::{Gpx, read};

// Earth's radius in meters.
const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

const WGS84_A: f64 = 6378137.0; // Semi-major axis
const WGS84_F: f64 = 1.0 / 298.257223563; // Flattening
const WGS84_E_SQ: f64 = WGS84_F * (2.0 - WGS84_F); // Eccentricity squared

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point
{
   pub lat: f64,
   pub lon: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint
{
   pub distance: f64, // Cumulative distance in meters
   pub point:    Point,
   pub heading:  f64, // Bearing/heading in degrees (0-360)
   pub altitude: f64
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ECEFCoord
{
   x: f64,
   y: f64,
   z: f64,
}

#[derive(Debug, Clone, Copy)]
pub enum DistanceMethod
{
   Haversine,
   ECEF,
}

/// Calculates the distance between two GPS coordinates using the Haversine formula.
/// Returns the distance in meters.
fn haversine_distance(p1: Point, p2: Point) -> f64
//------------------------------------------------
{
   let lat1_rad = p1.lat.to_radians();
   let lon1_rad = p1.lon.to_radians();
   let lat2_rad = p2.lat.to_radians();
   let lon2_rad = p2.lon.to_radians();

   let d_lat = lat2_rad - lat1_rad;
   let d_lon = lon2_rad - lon1_rad;

   let a = (d_lat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (d_lon / 2.0).sin().powi(2);
   let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

   EARTH_RADIUS_METERS * c
}

fn geodetic_to_ecef(p: Point) -> ECEFCoord
//----------------------------------------
{
   let lat_rad = p.lat.to_radians();
   let lon_rad = p.lon.to_radians();

   // Prime vertical radius of curvature
   let n = WGS84_A / (1.0 - WGS84_E_SQ * lat_rad.sin().powi(2)).sqrt();

   // h (height) is assumed to be 0
   let x = n * lat_rad.cos() * lon_rad.cos();
   let y = n * lat_rad.cos() * lon_rad.sin();
   let z = (n * (1.0 - WGS84_E_SQ)) * lat_rad.sin();

   ECEFCoord { x, y, z }
}

/// Calculates distance by converting to ECEF coordinates (ellipsoidal Earth).
fn ECEF_distance(p1: Point, p2: Point) -> f64
//-------------------------------------------
{
   let ecef1 = geodetic_to_ecef(p1);
   let ecef2 = geodetic_to_ecef(p2);

   // Simple Euclidean distance between the two 3D points
   ((ecef2.x - ecef1.x).powi(2) + (ecef2.y - ecef1.y).powi(2) + (ecef2.z - ecef1.z).powi(2)).sqrt()
}

pub fn build_track_data(path: &Path, method: DistanceMethod) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>>
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
   let mut last_point: Option<Point> = None;

   for point in &track_segment.points
   {
      let current_point = Point { lat: point.point().y(), lon: point.point().x(), };
      let current_altitude = point.elevation.unwrap_or(0.0);
      let mut current_heading = 0.0;

      if let Some(prev_point) = last_point
      {
         let segment_distance = match method
         {
            | DistanceMethod::Haversine => haversine_distance(prev_point, current_point),
            | DistanceMethod::ECEF => ECEF_distance(prev_point, current_point),
         };
         cumulative_distance += segment_distance;
         current_heading = calculate_bearing(prev_point.lat, prev_point.lon, current_point.lat, current_point.lon);
      }

      track_data.push(TrackPoint {  distance: cumulative_distance,
                                    point:    current_point,
                                    heading:  current_heading,
                                    altitude: current_altitude
                                 });

      last_point = Some(current_point);
   }

   Ok(track_data)
}

pub fn process_gpx(file_path: &str, method: DistanceMethod) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>>
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
   let track = match build_track_data(gpx_file_path, method)
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

fn calculate_bearing(from_latitude: f64, from_longitude: f64, to_latitude: f64, to_longitude: f64) -> f64
//-------------------------------------------------------------
{
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
