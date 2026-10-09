use std::fs;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct SystemMetrics {
    // 电池与功耗
    pub battery_cap: u32,
    pub battery_status: String,
    pub battery_volt: f32,
    pub battery_temp: f32,
    pub battery_wh: f32,       // 瓦时 (Wh)
    pub battery_power_w: f32,  // 实时功率 (W)

    // CPU 与系统
    pub cpu_temp: f32,
    pub cpu_percent: f32,
    pub cpu_load: String,
    pub uptime_str: String,

    // 内存与 Swap
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub mem_percent: f32,
    pub swap_used_mb: u64,
    pub swap_total_mb: u64,
    pub swap_percent: f32,

    // 存储
    pub disk_used_gb: f32,
    pub disk_total_gb: f32,
    pub disk_percent: f32,

    // 网络
    pub wifi_ssid: String,
    pub lan_ip: String,
    pub wan_ip: String,
    pub rx_mb: f64,
    pub tx_mb: f64,
}

pub struct MetricsCollector {
    cpu_percent_cache: Arc<Mutex<f32>>,
    wan_ip_cache: Arc<Mutex<String>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        let cpu_percent_cache = Arc::new(Mutex::new(0.0f32));
        let cpu_clone = cpu_percent_cache.clone();

        // 启动后台线程严格每 1000ms 采样一次 CPU，彻底隔离主线程事件中断与窗口坍塌
        thread::spawn(move || {
            let mut last_total = 0u64;
            let mut last_idle = 0u64;

            // 初始读取
            if let Ok(content) = fs::read_to_string("/proc/stat") {
                if let Some(line) = content.lines().next() {
                    if line.starts_with("cpu ") {
                        let parts: Vec<u64> = line[4..]
                            .split_whitespace()
                            .filter_map(|s| s.parse().ok())
                            .collect();
                        if parts.len() >= 4 {
                            last_total = parts.iter().sum();
                            last_idle = parts[3] + parts.get(4).copied().unwrap_or(0);
                        }
                    }
                }
            }

            loop {
                thread::sleep(Duration::from_millis(1000));
                if let Ok(content) = fs::read_to_string("/proc/stat") {
                    if let Some(line) = content.lines().next() {
                        if line.starts_with("cpu ") {
                            let parts: Vec<u64> = line[4..]
                                .split_whitespace()
                                .filter_map(|s| s.parse().ok())
                                .collect();
                            if parts.len() >= 4 {
                                let total: u64 = parts.iter().sum();
                                let idle: u64 = parts[3] + parts.get(4).copied().unwrap_or(0);
                                let diff_total = total.saturating_sub(last_total);
                                let diff_idle = idle.saturating_sub(last_idle);
                                last_total = total;
                                last_idle = idle;

                                if diff_total > 0 {
                                    let usage = 100.0 * (1.0 - (diff_idle as f32 / diff_total as f32));
                                    if let Ok(mut lock) = cpu_clone.lock() {
                                        *lock = usage.clamp(0.0, 100.0);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let wan_ip_cache = Arc::new(Mutex::new("获取中...".to_string()));
        let wan_ip_clone = wan_ip_cache.clone();

        // 启动后台线程异步获取公网 IP 并定期刷新 (每 10 分钟)
        thread::spawn(move || loop {
            let output = std::process::Command::new("/usr/bin/curl")
                .args(["-s", "-4", "-m", "3", "http://icanhazip.com"])
                .output()
                .or_else(|_| {
                    std::process::Command::new("/usr/bin/curl")
                        .args(["-s", "-4", "-m", "3", "http://myip.ipip.net"])
                        .output()
                })
                .or_else(|_| {
                    std::process::Command::new("/usr/bin/curl")
                        .args(["-s", "-4", "-m", "3", "https://api.ipify.org"])
                        .output()
                });

            if let Ok(out) = output {
                let raw_ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let ip = if raw_ip.contains("当前 IP：") {
                    raw_ip.split("当前 IP：").nth(1).and_then(|s| s.split_whitespace().next()).unwrap_or("").to_string()
                } else {
                    raw_ip.lines().next().unwrap_or("").trim().to_string()
                };

                if !ip.is_empty() && (ip.contains('.') || ip.contains(':')) {
                    if let Ok(mut lock) = wan_ip_clone.lock() {
                        *lock = ip;
                    }
                }
            }
            thread::sleep(Duration::from_secs(600));
        });

        Self {
            cpu_percent_cache,
            wan_ip_cache,
        }
    }

    pub fn collect(&mut self) -> SystemMetrics {
        let mut m = SystemMetrics::default();

        // 1. CPU 利用率 (严格读取后台 1 秒平滑时间窗口采样结果，无论如何点击屏幕绝不产生瞬态虚高)
        if let Ok(lock) = self.cpu_percent_cache.lock() {
            m.cpu_percent = *lock;
        }

        // 读取当前真实 CPU 主频 (去除8核废话)
        let mut cur_freq_ghz = 1.0f32;
        if let Ok(freq_str) = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq") {
            if let Ok(khz) = freq_str.trim().parse::<f32>() {
                cur_freq_ghz = (khz / 1000000.0 * 10.0).round() / 10.0;
            }
        }
        m.cpu_load = format!("{:.1} GHz", cur_freq_ghz);

        if let Ok(temp_str) = fs::read_to_string("/sys/class/thermal/thermal_zone1/temp") {
            if let Ok(raw) = temp_str.trim().parse::<f32>() {
                m.cpu_temp = raw / 1000.0;
            }
        }

        // 2. 电池与功耗 (死命令：严格只展示瓦时 Wh 与功率 W，去除电流)
        if let Ok(uevent) = fs::read_to_string("/sys/class/power_supply/battery/uevent") {
            let mut v_now: f32 = 4.0;
            let mut c_now: f32 = 0.0;
            for line in uevent.lines() {
                if let Some(val) = line.strip_prefix("POWER_SUPPLY_CAPACITY=") {
                    m.battery_cap = val.parse().unwrap_or(0);
                } else if let Some(val) = line.strip_prefix("POWER_SUPPLY_VOLTAGE_NOW=") {
                    if let Ok(raw) = val.parse::<f32>() {
                        v_now = raw / 1000000.0;
                        m.battery_volt = v_now;
                    }
                } else if let Some(val) = line.strip_prefix("POWER_SUPPLY_CURRENT_NOW=") {
                    if let Ok(raw) = val.parse::<f32>() {
                        c_now = raw; // mA
                    }
                } else if let Some(val) = line.strip_prefix("POWER_SUPPLY_BATT_TEMP=") {
                    if let Ok(raw) = val.parse::<f32>() {
                        m.battery_temp = raw / 10.0;
                    }
                } else if let Some(val) = line.strip_prefix("POWER_SUPPLY_STATUS=") {
                    m.battery_status = match val.trim() {
                        "Charging" => "充电中".to_string(),
                        "Full" => "已充满".to_string(),
                        _ => "放电中".to_string(),
                    };
                }
            }
            // 额定容量参考计算: 4000mAh * 3.85V = 15.4 Wh (后续版本支持从配置文件读取)
            m.battery_wh = (15.4 * (m.battery_cap as f32 / 100.0) * 10.0).round() / 10.0;
            m.battery_power_w = ((v_now * (c_now.abs() / 1000.0)) * 100.0).round() / 100.0;
        }

        // 3. 内存与 Swap
        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            let mut total_kb = 0u64;
            let mut avail_kb = 0u64;
            let mut swap_total_kb = 0u64;
            let mut swap_free_kb = 0u64;

            for line in meminfo.lines() {
                if let Some(v) = line.strip_prefix("MemTotal:") {
                    total_kb = v.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if let Some(v) = line.strip_prefix("MemAvailable:") {
                    avail_kb = v.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if let Some(v) = line.strip_prefix("SwapTotal:") {
                    swap_total_kb = v.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                } else if let Some(v) = line.strip_prefix("SwapFree:") {
                    swap_free_kb = v.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
                }
            }

            m.mem_total_mb = total_kb / 1024;
            let used_kb = total_kb.saturating_sub(avail_kb);
            m.mem_used_mb = used_kb / 1024;
            if total_kb > 0 {
                m.mem_percent = ((used_kb as f32 / total_kb as f32) * 100.0).clamp(0.0, 100.0);
            }

            m.swap_total_mb = swap_total_kb / 1024;
            let swap_used_kb = swap_total_kb.saturating_sub(swap_free_kb);
            m.swap_used_mb = swap_used_kb / 1024;
            if swap_total_kb > 0 {
                m.swap_percent = ((swap_used_kb as f32 / swap_total_kb as f32) * 100.0).clamp(0.0, 100.0);
            }
        }

        // 4. 存储空间 (/)
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            let path = std::ffi::CString::new("/").unwrap();
            if libc::statvfs(path.as_ptr(), &mut stat) == 0 {
                let total_bytes = stat.f_blocks as f64 * stat.f_frsize as f64;
                let free_bytes = stat.f_bfree as f64 * stat.f_frsize as f64;
                let used_bytes = total_bytes - free_bytes;
                m.disk_total_gb = (total_bytes / (1024.0 * 1024.0 * 1024.0)) as f32;
                m.disk_used_gb = (used_bytes / (1024.0 * 1024.0 * 1024.0)) as f32;
                if total_bytes > 0.0 {
                    m.disk_percent = ((used_bytes / total_bytes) * 100.0) as f32;
                }
            }
        }

        // 5. WiFi SSID
        m.wifi_ssid = Self::get_wifi_ssid();

        // 6. 局域网 IP
        m.lan_ip = Self::get_lan_ip();

        // 7. 公网 IP (来自缓存)
        if let Ok(lock) = self.wan_ip_cache.lock() {
            m.wan_ip = lock.clone();
        }

        // 8. 网络累计流量
        if let Ok(dev) = fs::read_to_string("/proc/net/dev") {
            for line in dev.lines() {
                if line.contains("wlan0:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 10 {
                        let rx: f64 = parts[1].parse().unwrap_or(0.0);
                        let tx: f64 = parts[9].parse().unwrap_or(0.0);
                        m.rx_mb = rx / (1024.0 * 1024.0);
                        m.tx_mb = tx / (1024.0 * 1024.0);
                    }
                    break;
                }
            }
        }

        // 9. Uptime
        if let Ok(uptime) = fs::read_to_string("/proc/uptime") {
            if let Some(sec_str) = uptime.split_whitespace().next() {
                if let Ok(secs) = sec_str.parse::<f64>() {
                    let d = (secs / 86400.0) as u32;
                    let h = ((secs % 86400.0) / 3600.0) as u32;
                    let min = ((secs % 3600.0) / 60.0) as u32;
                    if d > 0 {
                        m.uptime_str = format!("{}天 {}时 {}分", d, h, min);
                    } else {
                        m.uptime_str = format!("{}小时 {}分钟", h, min);
                    }
                }
            }
        }

        m
    }

    fn get_wifi_ssid() -> String {
        let conf_paths = [
            "/proc/1/root/data/misc/wifi/wpa_supplicant.conf",
            "/data/misc/wifi/wpa_supplicant.conf",
        ];

        for path in &conf_paths {
            if let Ok(content) = fs::read_to_string(path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(val) = trimmed.strip_prefix("ssid=") {
                        let raw = val.trim();
                        if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                            return raw[1..raw.len() - 1].to_string();
                        } else if !raw.is_empty() && raw.len() % 2 == 0 {
                            // 尝试 Hex 解码 (例如 434d4d59435ae59b9be6a5bc20403547487a)
                            let mut bytes = Vec::new();
                            let mut i = 0;
                            let mut ok = true;
                            while i < raw.len() {
                                if let Ok(b) = u8::from_str_radix(&raw[i..i + 2], 16) {
                                    bytes.push(b);
                                } else {
                                    ok = false;
                                    break;
                                }
                                i += 2;
                            }
                            if ok {
                                if let Ok(s) = String::from_utf8(bytes) {
                                    return s;
                                }
                            }
                        }
                    }
                }
            }
        }
        "未连接".to_string()
    }

    fn get_lan_ip() -> String {
        if let Ok(output) = std::process::Command::new("ip")
            .args(["-4", "-o", "addr", "show", "wlan0"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for part in text.split_whitespace() {
                if part.contains('/') && part.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    if let Some(ip) = part.split('/').next() {
                        return ip.to_string();
                    }
                }
            }
        }
        "未获取".to_string()
    }
}
