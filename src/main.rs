use palette::{FromColor, Hsl, RgbHue, Srgb};
use std::process::Command;
use colored::Colorize;
use xcap::Monitor;
use regex::Regex;
use lerp::Lerp;
use std::time::{Duration, Instant};
use std::thread;
use std::sync::{Arc, Mutex};

const VERBOSE: bool = false;

const MINIMUM_SATURATION_THRESHOLD: f32 = 0.6;
const MINIMUM_SATURATION_CORRECTING: f32 = 0.2;
const MINIMUM_LIGHTNESS_CORRECTING: f32 = 0.4;

const LERP_FACTOR: f32 = 0.05;
const LERP_INTERVAL: u64 = 16;
const UPDATE_INTERVAL: u64 = 256;

fn main() {
    
    // Wrap final_color in Arc<Mutex>
    let final_color = Arc::new(Mutex::new(u8_tuple_to_f32(
        average_rgb_hsl(&capture_dominant_colors(), 0.8, 0.4)
    )));
    
    // Clone the Arc for the thread
    let thread_final_color = Arc::clone(&final_color);
    
    thread::spawn(move || {
        loop {
            let new_color = u8_tuple_to_f32(average_rgb_hsl(&capture_dominant_colors(), MINIMUM_SATURATION_CORRECTING, MINIMUM_LIGHTNESS_CORRECTING));
            *thread_final_color.lock().unwrap() = new_color; // Update shared state
            thread::sleep(Duration::from_millis(UPDATE_INTERVAL));
        }
    });

    let mut current_color = *final_color.lock().unwrap();
    loop {
        // Get latest color from shared state
        let latest_color = *final_color.lock().unwrap();
        
        // Lerp towards latest color
        current_color.0 = current_color.0.lerp(latest_color.0, LERP_FACTOR);
        current_color.1 = current_color.1.lerp(latest_color.1, LERP_FACTOR);
        current_color.2 = current_color.2.lerp(latest_color.2, LERP_FACTOR);

        set_border_color(f32_tuple_to_u8(current_color));
        thread::sleep(Duration::from_millis(LERP_INTERVAL));
    }
}

fn to_hypr_color((r, g, b): (u8, u8, u8)) -> String {
    format!("\"rgba({:02x}{:02x}{:02x}ff)\"", r, g, b)
}

fn u8_tuple_to_f32(x: (u8, u8, u8)) -> (f32, f32, f32) {
    (
        f32::from(x.0),
        f32::from(x.1),
        f32::from(x.2)
    )
}

fn f32_tuple_to_u8(x: (f32, f32, f32)) -> (u8, u8, u8) {
    (
        x.0 as u8,
        x.1 as u8,
        x.2 as u8,
    )
}

fn average_rgb_hsl(colors: &[(u8, u8, u8)], minimum_s: f32, minimum_l: f32) -> (u8, u8, u8) {
    if colors.is_empty() {
        return (0, 0, 0);
    }

    let count = colors.len() as f32;
    let mut sum_cos = 0.0;
    let mut sum_sin = 0.0;
    let mut sum_saturation = 0.0;
    let mut sum_lightness = 0.0;

    for &(r, g, b) in colors {
        let srgb = Srgb::new(r, g, b).into_format();
        let hsl: Hsl = Hsl::from_color(srgb);

        let hue_degrees = hsl.hue.into_degrees();
        let hue_radians = hue_degrees.to_radians();
        sum_cos += hue_radians.cos();
        sum_sin += hue_radians.sin();

        sum_saturation += hsl.saturation;
        sum_lightness += hsl.lightness;
    }

    let mut avg_saturation = (sum_saturation / count).clamp(0.0, 1.0);
    let mut avg_lightness = (sum_lightness / count).clamp(0.0, 1.0);

    let avg_cos = sum_cos / count;
    let avg_sin = sum_sin / count;
    let avg_hue_radians = avg_sin.atan2(avg_cos);
    let avg_hue_degrees = avg_hue_radians.to_degrees().rem_euclid(360.0);

    if avg_saturation < minimum_s {
        avg_saturation = minimum_s;
    }
    if avg_lightness < minimum_l {
        avg_lightness = minimum_l;
    }

    let avg_hsl = Hsl::new(
        RgbHue::from_degrees(avg_hue_degrees),
        avg_saturation,
        avg_lightness,
    );

    let avg_rgb: Srgb = Srgb::from_color(avg_hsl);
    let avg_rgb_u8 = avg_rgb.into_format();

    (
        avg_rgb_u8.red,
        avg_rgb_u8.green,
        avg_rgb_u8.blue,
    )
}

fn set_border_color(color: (u8, u8, u8)) {
    let hyprctl_call = format!("hyprctl keyword general:col.active_border {}", to_hypr_color(color));
    if VERBOSE {
        println!("Executing command: {}", hyprctl_call.truecolor(color.0, color.1, color.2));
    }

    if cfg!(target_os = "windows") {
        eprintln!("This program is intended for Linux with Hyprland");
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(&hyprctl_call)
            .output()
            .expect("Failed to execute hyprctl command");
    }
}

fn extract_monitor_id() -> Option<String> {
    if cfg!(target_os = "windows") {
        println!("Please run this under hyprland + Linux!!");
        return None;
    }

    let output = Command::new("sh")
        .arg("-c")
        .arg("hyprctl activeworkspace")
        .output()
        .expect("failed to execute command");

    let output_str = String::from_utf8(output.stdout)
        .expect("command output is not valid UTF-8");

    let re = Regex::new(r"on monitor ([^:]+):").unwrap();
    re.captures(&output_str)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}

fn capture_dominant_colors() -> Vec<(u8, u8, u8)> {
    let monitors = Monitor::all().expect("Failed to get monitors");
    let mut dominant_colors = Vec::new();
    let active_monitor = extract_monitor_id();

    for monitor in monitors {
        let is_active_monitor = match (monitor.name(), active_monitor.clone()) {
            (std::result::Result::Ok(a), Some(b)) => a == b,
            _ => false,
        };
        if !is_active_monitor {
            if VERBOSE {
                println!("Monitor {:?} is not the Active Monitor -- skipping", monitor.name());
            }
            continue;
        }

        let capture_start = Instant::now();
        let image = monitor.capture_image().unwrap();
        if VERBOSE {
            println!(
                "Monitor {:?} captured in {:?}",
                monitor.name(),
                capture_start.elapsed()
            );
        }

        let (sum_r, sum_g, sum_b, count) = image.pixels()
            .filter_map(|pixel| {
                let r = pixel[0];
                let g = pixel[1];
                let b = pixel[2];

                let srgb = Srgb::new(r, g, b);
                let srgb_f32 = srgb.into_format::<f32>();
                let hsl = Hsl::from_color(srgb_f32);

                (hsl.saturation > MINIMUM_SATURATION_THRESHOLD).then_some((r as u128, g as u128, b as u128))
            })
            .fold((0, 0, 0, 0), |(sr, sg, sb, c), (r, g, b)| {
                (sr + r, sg + g, sb + b, c + 1)
            });

        if count == 0 {
            println!("No saturated pixels found for monitor");
            continue;
        }

        let average_r = (sum_r / count) as u8;
        let average_g = (sum_g / count) as u8;
        let average_b = (sum_b / count) as u8;

        if VERBOSE {
            println!(
                "    {} {:?}",
                "Dominant color for Monitor".truecolor(average_r, average_g, average_b), monitor.name()
            );
        }

        dominant_colors.push((average_r, average_g, average_b));
    }

    dominant_colors
}
