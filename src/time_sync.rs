use std::fs;
use std::net::UdpSocket;
use std::os::unix::fs::symlink;
use std::thread;
use std::time::Duration;

pub fn init_time_sync() {
    // 1. 设置时区为 Asia/Shanghai (UTC+8)
    ensure_timezone();

    // 2. 启动后台线程执行网络自动校时
    thread::spawn(|| {
        loop {
            if sync_time_from_ntp_or_http() {
                // 校准成功后每 2 小时复核一次
                thread::sleep(Duration::from_secs(7200));
            } else {
                // 如果失败，10 秒后重试
                thread::sleep(Duration::from_secs(10));
            }
        }
    });
}

fn ensure_timezone() {
    let target = "/usr/share/zoneinfo/Asia/Shanghai";
    let link = "/etc/localtime";
    if fs::metadata(target).is_ok() {
        let _ = fs::remove_file(link);
        let _ = symlink(target, link);
        let _ = fs::write("/etc/timezone", "Asia/Shanghai\n");
    }
}

fn sync_time_from_ntp_or_http() -> bool {
    // 优先尝试 NTP (ntp.aliyun.com)
    if let Some(remote_sec) = fetch_ntp_timestamp("ntp.aliyun.com:123") {
        return apply_system_time(remote_sec);
    }
    // 备用 NTP (pool.ntp.org)
    if let Some(remote_sec) = fetch_ntp_timestamp("pool.ntp.org:123") {
        return apply_system_time(remote_sec);
    }
    // 备用 HTTP (淘宝时间戳 API: 秒级绝对时间)
    if let Some(remote_sec) = fetch_http_timestamp() {
        return apply_system_time(remote_sec);
    }
    false
}

fn fetch_ntp_timestamp(server: &str) -> Option<u64> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    socket.set_write_timeout(Some(Duration::from_secs(3))).ok()?;

    // 构造 NTP v4 请求报文 (48 字节, LI=0, VN=4, Mode=3 Client)
    let mut packet = [0u8; 48];
    packet[0] = 0x23;

    socket.send_to(&packet, server).ok()?;
    let (amt, _) = socket.recv_from(&mut packet).ok()?;
    if amt < 48 {
        return None;
    }

    // 提取 Transmit Timestamp (字节 40..44 为秒数，基于 1900-01-01)
    let secs_1900 = u32::from_be_bytes(packet[40..44].try_into().ok()?) as u64;
    // 减去 1900 到 1970 之间的 70 年秒数 (2208988800)
    if secs_1900 > 2208988800 {
        Some(secs_1900 - 2208988800)
    } else {
        None
    }
}

fn fetch_http_timestamp() -> Option<u64> {
    let output = std::process::Command::new("curl")
        .args([
            "-s",
            "-m",
            "3",
            "http://api.m.taobao.com/rest/api3.do?api=mtop.common.getTimestamp",
        ])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    if let Some(pos) = text.find("\"t\":\"") {
        let rest = &text[pos + 5..];
        if let Some(end) = rest.find('"') {
            let ms_str = &rest[..end];
            if let Ok(ms) = ms_str.parse::<u64>() {
                return Some(ms / 1000);
            }
        }
    }
    None
}

fn apply_system_time(remote_sec: u64) -> bool {
    let mut now: libc::timeval = unsafe { std::mem::zeroed() };
    unsafe {
        libc::gettimeofday(&mut now, std::ptr::null_mut());
    }

    let diff = (remote_sec as i64 - now.tv_sec as i64).abs();
    if diff > 1 {
        let new_time = libc::timeval {
            tv_sec: remote_sec as libc::time_t,
            tv_usec: 0,
        };
        unsafe {
            if libc::settimeofday(&new_time, std::ptr::null()) == 0 {
                println!(
                    "自动校时成功: 纠偏 {} 秒，系统内核时间已同步为 {}",
                    diff, remote_sec
                );
                return true;
            }
        }
    } else {
        return true;
    }
    false
}
