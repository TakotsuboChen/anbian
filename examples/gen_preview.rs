#[path = "../src/framebuffer.rs"]
mod framebuffer;
#[path = "../src/metrics.rs"]
mod metrics;
#[path = "../src/render.rs"]
mod render;

use metrics::SystemMetrics;
use render::{FrameBufferPool, Renderer};
use std::fs;

fn main() {
    let renderer = Renderer::new();
    let mut pool = FrameBufferPool::new();
    let metrics = SystemMetrics {
        battery_cap: 82,
        battery_status: "放电中".to_string(),
        battery_volt: 4.12,
        battery_temp: 31.0,
        battery_wh: 12.6,
        battery_power_w: 1.54,
        cpu_temp: 36.6,
        cpu_percent: 24.0,
        cpu_load: "0.9 GHz".to_string(),
        uptime_str: "6小时 5分钟".to_string(),
        mem_used_mb: 727,
        mem_total_mb: 2792,
        mem_percent: 26.0,
        swap_used_mb: 0,
        swap_total_mb: 0,
        swap_percent: 0.0,
        disk_used_gb: 5.1,
        disk_total_gb: 24.9,
        disk_percent: 20.5,
        wifi_ssid: "AnBian-Home-5G".to_string(),
        lan_ip: "192.168.1.188".to_string(),
        wan_ip: "203.0.113.88".to_string(),
        rx_mb: 238.4,
        tx_mb: 50.2,
    };
    let data = renderer.render(&mut pool, &metrics);
    fs::write("preview_raw.bin", data).unwrap();
    println!("preview_raw.bin generated successfully");
}
