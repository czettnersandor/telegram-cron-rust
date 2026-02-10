#!/usr/bin/env python3
"""
weather.py — fetch current weather using Open-Meteo (free, no API key needed).
Returns NOUPDATE on error (silent fail), or a formatted weather report.

Usage:
  weather.py --lat 52.3676 --lon 4.9041 --city Amsterdam
"""

import argparse
import sys
import urllib.request
import urllib.error
import json

WMO_CODES = {
    0: "Clear sky ☀️",
    1: "Mainly clear 🌤️", 2: "Partly cloudy ⛅", 3: "Overcast ☁️",
    45: "Foggy 🌫️", 48: "Icy fog 🌫️",
    51: "Light drizzle 🌦️", 53: "Moderate drizzle 🌦️", 55: "Dense drizzle 🌧️",
    61: "Slight rain 🌧️", 63: "Moderate rain 🌧️", 65: "Heavy rain 🌧️",
    71: "Slight snow 🌨️", 73: "Moderate snow 🌨️", 75: "Heavy snow ❄️",
    77: "Snow grains ❄️",
    80: "Slight showers 🌦️", 81: "Moderate showers 🌧️", 82: "Violent showers ⛈️",
    85: "Slight snow showers 🌨️", 86: "Heavy snow showers ❄️",
    95: "Thunderstorm ⛈️", 96: "Thunderstorm + hail ⛈️", 99: "Thunderstorm + heavy hail ⛈️",
}


def fetch_weather(lat: float, lon: float) -> dict:
    url = (
        f"https://api.open-meteo.com/v1/forecast"
        f"?latitude={lat}&longitude={lon}"
        f"&current=temperature_2m,apparent_temperature,relative_humidity_2m,"
        f"wind_speed_10m,wind_direction_10m,weathercode,precipitation"
        f"&daily=temperature_2m_max,temperature_2m_min,precipitation_sum,"
        f"weathercode,sunrise,sunset"
        f"&timezone=auto&forecast_days=1"
    )
    req = urllib.request.Request(url, headers={"User-Agent": "telegram-cron-weather/1.0"})
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def wind_direction(deg: float) -> str:
    dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"]
    return dirs[round(deg / 45) % 8]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--lat", type=float, required=True)
    parser.add_argument("--lon", type=float, required=True)
    parser.add_argument("--city", type=str, default="")
    args = parser.parse_args()

    try:
        data = fetch_weather(args.lat, args.lon)
    except Exception as e:
        # Silent fail: don't spam Telegram on network issues
        print("NOUPDATE")
        sys.exit(0)

    c = data["current"]
    d = data["daily"]

    condition = WMO_CODES.get(c["weathercode"], f"Code {c['weathercode']}")
    city_str = f" — {args.city}" if args.city else ""
    tz = data.get("timezone", "")
    sunrise = d["sunrise"][0].split("T")[1] if d.get("sunrise") else "?"
    sunset = d["sunset"][0].split("T")[1] if d.get("sunset") else "?"

    wind_dir = wind_direction(c["wind_direction_10m"])

    lines = [
        f"🌍 Weather{city_str}",
        f"🌡️ {c['temperature_2m']}°C (feels like {c['apparent_temperature']}°C)",
        f"💧 Humidity: {c['relative_humidity_2m']}%",
        f"💨 Wind: {c['wind_speed_10m']} km/h {wind_dir}",
        f"🌧️ Precipitation: {c['precipitation']} mm",
        f"☁️ {condition}",
        f"📅 Today: {d['temperature_2m_min'][0]}°C – {d['temperature_2m_max'][0]}°C, "
        f"rain {d['precipitation_sum'][0]} mm",
        f"🌅 Sunrise: {sunrise}  🌇 Sunset: {sunset}",
    ]
    if tz:
        lines.append(f"🕐 Timezone: {tz}")

    print("\n".join(lines))


if __name__ == "__main__":
    main()
