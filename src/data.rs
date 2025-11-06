use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiderDataJSON 
{
    pub name: String,
    pub country: String,
    pub team: String,
    #[serde(rename = "teamCode")]
    pub team_code: String,
    pub power: i32,
    #[serde(rename = "avgPower")]
    pub avg_power: i32,
    #[serde(rename = "nrmPower")]
    pub nrm_power: i32,
    #[serde(rename = "maxPower")]
    pub max_power: i32,
    pub cadence: i32,
    #[serde(rename = "avgCadence")]
    pub avg_cadence: i32,
    #[serde(rename = "maxCadence")]
    pub max_cadence: i32,
    pub heartrate: i32,
    #[serde(rename = "avgHeartrate")]
    pub avg_heartrate: i32,
    #[serde(rename = "maxHeartrate")]
    pub max_heartrate: i32,
    pub time: i32,
    pub distance: i32,
    pub height: i32,
    pub speed: i32, // speed in millimetres per second. 1 mm/s = 0.0036 km/h, 
    pub tss: i32,
    pub calories: i32,
    pub draft: i32,
    #[serde(rename = "windSpeed")]
    pub wind_speed: i32, // wind speed in millimetres per second !!
    #[serde(rename = "windAngle")]
    pub wind_angle: i32,
    pub slope: i32,
    #[serde(rename = "eventLapsTotal")]
    pub event_laps_total: i32,
    #[serde(rename = "eventLapsDone")]
    pub event_laps_done: i32,
    #[serde(rename = "eventDistanceTotal")]
    pub event_distance_total: i32,
    #[serde(rename = "eventDistanceDone")]
    pub event_distance_done: i32,
    #[serde(rename = "eventDistanceToNextLocation")]
    pub event_distance_to_next_location: i32,
    #[serde(rename = "eventNextLocation")]
    pub event_next_location: i32,
    #[serde(rename = "eventPosition")]
    pub event_position: i32,
    #[serde(skip)]
    pub latitude: f64,
    #[serde(skip)]
    pub longitude: f64, 
    #[serde(skip)]
    pub altitude: f64
}


impl Default for RiderDataJSON 
{
    fn default() -> Self 
    {
        Self 
        {
            name: String::new(),
            country: String::new(),
            team: String::new(),
            team_code: String::new(),
            power: 0,
            avg_power: 0,
            nrm_power: 0,
            max_power: 0,
            cadence: 0,
            avg_cadence: 0,
            max_cadence: 0,
            heartrate: 0,
            avg_heartrate: 0,
            max_heartrate: 0,
            time: 0,
            distance: 0,
            height: 0,
            speed: 0,
            tss: 0,
            calories: 0,
            draft: 0,
            wind_speed: 0,
            wind_angle: 0,
            slope: 0,
            event_laps_total: 0,
            event_laps_done: 0,
            event_distance_total: 0,
            event_distance_done: 0,
            event_distance_to_next_location: 0,
            event_next_location: 0,
            event_position: 0,

            latitude: 0.0,
            longitude: 0.0,
            altitude: 0.0   
        }
    }
}

impl RiderDataJSON 
{
    pub fn from_json(json_str: &str) -> Result<Self, String> 
    {
        serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse rider data JSON: {}", e))
    }

    pub fn to_json(&self) -> Result<String, String> 
    {
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize rider data to JSON: {}", e))
    }

    pub fn distance_meters(&self) -> f64 { self.distance as f64 }

    /// Get the rider's current speed in km/h (converts from m/s -> km/h)
    pub fn speed_kmh(&self) -> f64 { self.speed as f64 / 1000.0 * 3.6 }

    /// Get the distance in kilometers
    pub fn distance_km(&self) -> f64 { self.distance as f64 / 1000.0 }

    /// Check if the rider is currently pedaling (has current power output)
    pub fn is_pedaling(&self) -> bool { self.power > 0 }

    /// Get wind speed in km/h
    pub fn wind_speed_kmh(&self) -> f64 { self.wind_speed as f64 / 1000.0 * 3.6 }

    pub fn wind_direction_degrees(&self) -> f64 
    //--------------------------------------------
    {
        let angle = self.wind_angle as f64;
        if angle < 0.0 
        {
            360.0 + angle
        } else {
            angle
        }
    }
}


pub fn parse_rider_json(json_str: &str) -> Result<RiderDataJSON, String> { RiderDataJSON::from_json(json_str) }

/// No Strings makes Copy possible for use in AtomicCell (and we're only dealing with one rider anyway so names needed).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RiderData
{    
    pub distance: i32,
    pub wind_angle: i32,
    pub wind_speed: i32,
    pub slope: i32,
    pub height: i32,
    pub latitude: f64,
    pub longitude: f64, 
    pub altitude: f64
}

impl From<RiderDataJSON> for RiderData
{
    fn from(rider: RiderDataJSON) -> Self
    {
        Self
        {
            distance: rider.distance,
            wind_angle: rider.wind_angle,
            wind_speed: rider.wind_speed,
            slope: rider.slope,
            height: rider.height,
            latitude: rider.latitude,
            longitude: rider.longitude,
            altitude: rider.altitude,
        }
    }
}

impl From<&RiderDataJSON> for RiderData
{
    fn from(rider: &RiderDataJSON) -> Self
    {
        Self
        {
            distance: rider.distance,
            wind_angle: rider.wind_angle,
            wind_speed: rider.wind_speed,
            slope: rider.slope,
            height: rider.height,
            latitude: rider.latitude,
            longitude: rider.longitude,
            altitude: rider.altitude,
        }
    }
}

impl Default for RiderData
{
    fn default() -> Self
    {
        Self
        {
            distance: 0,
            wind_angle: 0,
            wind_speed: 0,
            slope: 0,
            height: 0,
            latitude: 0.0,
            longitude: 0.0,
            altitude: 0.0,
        }
    }
}   
