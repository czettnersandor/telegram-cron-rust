#!/usr/bin/env python3
"""
Paramotor Flying Conditions Checker
Checks weather conditions for paramotor flying at specified coordinates in Hungary.
"""

import requests
from datetime import datetime, timedelta
import sys


def get_next_friday_monday():
    """Calculate the dates for the upcoming Friday and Monday."""
    today = datetime.now().date()
    current_weekday = today.weekday()  # Monday=0, Sunday=6

    # Calculate days until next Friday (4)
    if current_weekday <= 4:  # Monday to Friday
        days_to_friday = 4 - current_weekday
    else:  # Saturday or Sunday
        days_to_friday = (7 - current_weekday) + 4

    # Calculate days until next Monday (0)
    if current_weekday == 0:  # If today is Monday, get next Monday
        days_to_monday = 7
    else:
        days_to_monday = (7 - current_weekday) % 7
        if days_to_monday == 0:  # If result is 0, we want next Monday
            days_to_monday = 7

    friday = today + timedelta(days=days_to_friday)
    monday = today + timedelta(days=days_to_monday)

    return friday, monday


def degrees_to_cardinal(degrees):
    """Convert wind direction in degrees to cardinal direction."""
    if degrees is None:
        return "N/A"

    directions = ['N', 'NNE', 'NE', 'ENE', 'E', 'ESE', 'SE', 'SSE',
                  'S', 'SSW', 'SW', 'WSW', 'W', 'WNW', 'NW', 'NNW']
    index = round(degrees / 22.5) % 16
    return f"{directions[index]} ({degrees:.0f}°)"


def fetch_weather_data(latitude, longitude, target_dates):
    """Fetch weather data from Open-Meteo API."""
    # Calculate date range
    start_date = min(target_dates)
    end_date = max(target_dates)

    url = "https://api.open-meteo.com/v1/forecast"
    params = {
        'latitude': latitude,
        'longitude': longitude,
        'hourly': 'precipitation,wind_speed_180m,wind_direction_180m',
        'start_date': start_date.strftime('%Y-%m-%d'),
        'end_date': end_date.strftime('%Y-%m-%d'),
        'timezone': 'Europe/Budapest'
    }

    try:
        response = requests.get(url, params=params, timeout=10)
        response.raise_for_status()
        return response.json()
    except requests.exceptions.RequestException as e:
        print(f"Error fetching weather data: {e}", file=sys.stderr)
        sys.exit(1)


def analyze_day(hourly_data, target_date):
    """Analyze weather conditions for a specific day."""
    times = hourly_data['time']
    wind_speeds = hourly_data['wind_speed_180m']
    wind_directions = hourly_data['wind_direction_180m']
    precipitation = hourly_data['precipitation']

    target_date_str = target_date.strftime('%Y-%m-%d')

    day_data = {
        'hours': [],
        'has_rain': False,
        'max_wind': 0,
        'min_wind': float('inf'),
        'flyable_hours': 0,
        'total_hours': 0
    }

    for i, time_str in enumerate(times):
        if time_str.startswith(target_date_str):
            hour = datetime.fromisoformat(time_str).hour
            wind = wind_speeds[i]
            wind_dir = wind_directions[i]
            precip = precipitation[i]

            day_data['hours'].append({
                'hour': hour,
                'wind': wind,
                'wind_direction': wind_dir,
                'precipitation': precip
            })

            day_data['total_hours'] += 1
            day_data['max_wind'] = max(day_data['max_wind'], wind)
            day_data['min_wind'] = min(day_data['min_wind'], wind)

            if precip > 0:
                day_data['has_rain'] = True

            # Check flyable conditions
            if precip == 0 and wind < 15:
                day_data['flyable_hours'] += 1

    return day_data


def is_potentially_flyable(day_data):
    """Determine if a day is potentially flyable."""
    # If there's no rain and wind is below 15 km/h for at least some hours
    if not day_data['has_rain'] and day_data['flyable_hours'] > 0:
        return True
    # If max wind is close to threshold (within 3 km/h) even if above, still report
    if not day_data['has_rain'] and day_data['max_wind'] < 18:
        return True
    return False


def format_report(day_name, date, day_data):
    """Format a report for a specific day."""
    report = []
    report.append(f"{day_name.upper()} - {date.strftime('%B %d, %Y')}")

    if day_data['has_rain']:
        report.append(f"⚠️  PRECIPITATION DETECTED")
    else:
        report.append(f"✓ No precipitation")

    report.append(f"\nWind speed at 180m:")
    report.append(f"  Minimum: {day_data['min_wind']:.1f} km/h")
    report.append(f"  Maximum: {day_data['max_wind']:.1f} km/h")

    if day_data['flyable_hours'] > 0:
        report.append(f"\n✓ {day_data['flyable_hours']} hours with flyable conditions (no rain + wind < 15 km/h)")
    else:
        report.append(f"\n⚠️  No hours with ideal flyable conditions")

    # Show hourly breakdown for daylight hours (6 AM to 8 PM)
    report.append(f"\nHourly forecast (daylight hours):")
    report.append(f"{'Hour':<6} {'Wind (km/h)':<15} {'Direction':<15} {'Precip (mm)':<12} {'Flyable?'}")

    for hour_data in day_data['hours']:
        hour = hour_data['hour']
        if 6 <= hour <= 20:  # Daylight hours
            wind = hour_data['wind']
            wind_dir = degrees_to_cardinal(hour_data['wind_direction'])
            precip = hour_data['precipitation']
            flyable = "✓" if (precip == 0 and wind < 15) else "✗"
            report.append(f"{hour:02d}:00  {wind:>6.1f}          {wind_dir:<15} {precip:>6.1f}        {flyable}")

    return "\n".join(report)


def main():
    if len(sys.argv) < 3:
        print("Error: Latitude and longitude required as arguments", file=sys.stderr)
        sys.exit(1)
    
    # Configuration
    LATITUDE = float(sys.argv[1])
    LONGITUDE = float(sys.argv[2])

    # Get target dates
    friday, monday = get_next_friday_monday()

    # Fetch weather data
    weather_data = fetch_weather_data(LATITUDE, LONGITUDE, [friday, monday])

    if 'hourly' not in weather_data:
        print("Error: Invalid weather data received", file=sys.stderr)
        sys.exit(1)

    # Analyze both days
    friday_data = analyze_day(weather_data['hourly'], friday)
    monday_data = analyze_day(weather_data['hourly'], monday)

    # Check if either day is potentially flyable
    friday_flyable = is_potentially_flyable(friday_data)
    monday_flyable = is_potentially_flyable(monday_data)

    if not friday_flyable and not monday_flyable:
        print("NOUPDATE")
        sys.exit(0)

    # Print report header
    print(f"PARAMOTOR FLYING CONDITIONS REPORT")
    print(f"Location: {LATITUDE}, {LONGITUDE} (Hungary)")
    print(f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")

    # Print reports for flyable days
    if friday_flyable:
        print(format_report("Friday", friday, friday_data))

    if monday_flyable:
        print(format_report("Monday", monday, monday_data))

    print("Note: Final decision should consider additional factors")
    print("(thermals, visibility, personal experience, etc.)")

if __name__ == "__main__":
    main()
