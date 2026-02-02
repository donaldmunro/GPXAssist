#!/usr/bin/env python3

import argparse
import json
import math
import os
import random
import signal
import sys
import time
import xml.etree.ElementTree as ET

# Constants for ECEF (WGS-84)
WGS84_A = 6378137.0  # semi-major axis
WGS84_F = 1 / 298.257223563  # flattening
WGS84_E2 = WGS84_F * (2 - WGS84_F)  # first eccentricity squared

def haversine(lat1, lon1, lat2, lon2):
    """Calculate Haversine distance in meters (ignores altitude)."""
    R = 6371000  # radius of Earth in meters
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlambda = math.radians(lon2 - lon1)
    a = math.sin(dphi / 2)**2 + math.cos(phi1) * math.cos(phi2) * math.sin(dlambda / 2)**2
    c = 2 * math.atan2(math.sqrt(a), math.sqrt(1 - a))
    return R * c

def lla_to_ecef(lat, lon, alt):
    """Convert Geodetic LLA to ECEF coordinates."""
    lat_rad = math.radians(lat)
    lon_rad = math.radians(lon)
    n = WGS84_A / math.sqrt(1 - WGS84_E2 * math.sin(lat_rad)**2)
    x = (n + alt) * math.cos(lat_rad) * math.cos(lon_rad)
    y = (n + alt) * math.cos(lat_rad) * math.sin(lon_rad)
    z = (n * (1 - WGS84_E2) + alt) * math.sin(lat_rad)
    return x, y, z

def ecef_distance(p1, p2):
    """Calculate Euclidean distance between two ECEF points (includes altitude)."""
    x1, y1, z1 = lla_to_ecef(*p1)
    x2, y2, z2 = lla_to_ecef(*p2)
    return math.sqrt((x2 - x1)**2 + (y2 - y1)**2 + (z2 - z1)**2)

def interpolate(p1, p2, ratio):
    """Linear interpolation between two LLA points."""
    lat = p1[0] + (p2[0] - p1[0]) * ratio
    lon = p1[1] + (p2[1] - p1[1]) * ratio
    alt = p1[2] + (p2[2] - p1[2]) * ratio
    return lat, lon, alt

def parse_gpx(file_path):
    """Parse GPX file and extract trkpt coordinates."""
    tree = ET.parse(file_path)
    root = tree.getroot()
    # Handle namespaces
    ns = {'gpx': 'http://www.topografix.com/GPX/1/1'}
    points = []
    for trkpt in root.findall('.//gpx:trkpt', ns):
        lat = float(trkpt.get('lat'))
        lon = float(trkpt.get('lon'))
        ele_node = trkpt.find('gpx:ele', ns)
        ele = float(ele_node.text) if ele_node is not None else 0.0
        points.append((lat, lon, ele))
    return points

def signal_handler(sig, frame):
    print("\nTermination signal received. Exiting...")
    sys.exit(0)

def main():
    parser = argparse.ArgumentParser(description="GPX Simulation for JSON updates.")
    parser.add_argument("gpx_path", help="Path to the source GIS GPX file.")
    parser.add_argument("json_path", help="Path to the JSON file to be updated.")
    parser.add_argument("-s", "--sleep", type=int, default=300, help="Pause between updates in ms.")
    parser.add_argument("-i", "--increment", type=int, default=2, help="Minimum distance between points in meters.")
    parser.add_argument("-m", "--method", choices=['ecef', 'haversine'], default='ecef', help="Distance calculation method.")

    args = parser.parse_args()

    # Setup signal handling
    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)

    points = parse_gpx(args.gpx_path)
    if not points:
        print("Error: No track points found in GPX file.")
        sys.exit(1)

    dist_func = ecef_distance if args.method == 'ecef' else lambda p1, p2: haversine(p1[0], p1[1], p2[0], p2[1])

    # Calculate total distance
    total_track_distance = 0
    for i in range(len(points) - 1):
        total_track_distance += dist_func(points[i], points[i+1])

    print(f"Total track distance: {total_track_distance:.2f} m")

    current_dist = 0.0
    wind_speed = random.randint(0, 4000)
    wind_angle = random.randint(0, 360)

    # Simulation loop
    point_idx = 0
    current_pos = points[0]

    while current_dist <= total_track_distance and point_idx < len(points) - 1:
        next_target = points[point_idx + 1]
        dist_to_next = dist_func(current_pos, next_target)

        if dist_to_next > args.increment:
            # Interpolate
            ratio = args.increment / dist_to_next
            current_pos = interpolate(current_pos, next_target, ratio)
            step_dist = args.increment
        else:
            # Move to next point
            current_pos = next_target
            step_dist = dist_to_next
            point_idx += 1

        current_dist += step_dist
        if current_dist > total_track_distance:
            current_dist = total_track_distance

        json_data = [{
            "name": "Donald Munro",
            "country": "RSA",
            "team": "indieVelo Founders Club",
            "teamCode": "IVFC",
            "power": random.randint(150, 400),
            "avgPower": 250,
            "nrmPower": 260,
            "maxPower": 1000,
            "cadence": random.randint(70, 110),
            "avgCadence": 90,
            "maxCadence": 120,
            "heartrate": random.randint(120, 180),
            "avgHeartrate": 150,
            "maxHeartrate": 190,
            "latitude": degrees_to_semicircles(current_pos[0]),
            "longitude": degrees_to_semicircles(current_pos[1]),
            "altitude": int(current_pos[2]*1000.0),
            "time": int(time.time()),
            "distance": int(current_dist),
            "height": int(current_pos[2]),
            "speed": random.randint(5, 15),
            "tss": 0,
            "calories": 0,
            "draft": 0,
            "windSpeed": wind_speed,
            "windAngle": wind_angle,
            "slope": random.randint(-5, 5),
            "brakeStrength": 0,
            "eventLapsTotal": 1,
            "eventLapsDone": -1,
            "eventDistanceTotal": int(total_track_distance),
            "eventDistanceDone": int(current_dist),
            "eventDistanceToNextLocation": 0,
            "eventNextLocation": 0,
            "eventPosition": 0
        }]

        # Atomic write
        temp_path = args.json_path + ".tmp"
        with open(temp_path, 'w') as f:
            json.dump(json_data, f, indent=3)
        os.replace(temp_path, args.json_path)

        time.sleep(args.sleep / 1000.0)

    print("Simulation complete.")

def degrees_to_semicircles(degrees):
    """Converts degrees to 32-bit integer semicircles."""
    return int((float(degrees) * 2147483648.0) / 180.0)

if __name__ == "__main__":
    main()
