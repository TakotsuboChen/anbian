use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::os::unix::io::AsRawFd;

pub const SCREEN_WIDTH: u32 = 1080;
pub const SCREEN_HEIGHT: u32 = 1920;
#[allow(dead_code)]
pub const VIRTUAL_WIDTH: u32 = 1088;
pub const LINE_STRIDE: u32 = 4352;
pub const PAGE_SIZE: usize = (LINE_STRIDE * SCREEN_HEIGHT) as usize; // 8,355,840 bytes

const FB_DEV: &str = "/dev/graphics/fb0";
const BACKLIGHT_DEV: &str = "/sys/class/leds/lcd-backlight/brightness";
const BLANK_DEV: &str = "/sys/class/graphics/fb0/blank";

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
const FBIOPAN_DISPLAY: libc::c_ulong = 0x4606;

pub struct Framebuffer {
    file: std::fs::File,
    pan_vinfo: [u8; 160],
    has_pan_vinfo: bool,
}

impl Framebuffer {
    pub fn new() -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(FB_DEV)
            .map_err(|e| format!("无法打开 Framebuffer 设备 {}: {}", FB_DEV, e))?;

        let mut pan_vinfo = [0u8; 160];
        let mut has_pan_vinfo = false;
        unsafe {
            let ret = libc::ioctl(file.as_raw_fd(), FBIOGET_VSCREENINFO, pan_vinfo.as_mut_ptr());
            if ret >= 0 {
                // 设置 xoffset = 0, yoffset = 0
                pan_vinfo[16..24].copy_from_slice(&0u64.to_ne_bytes());
                has_pan_vinfo = true;
            }
        }

        Ok(Self {
            file,
            pan_vinfo,
            has_pan_vinfo,
        })
    }

    pub fn write_triple_buffer(&mut self, data: &[u8]) {
        // MTK 硬件三重缓冲写入，彻底消除撕裂与历史残留帧
        let _ = self.file.write_all_at(data, 0);
        let _ = self.file.write_all_at(data, PAGE_SIZE as u64);
        let _ = self.file.write_all_at(data, (PAGE_SIZE * 2) as u64);

        if self.has_pan_vinfo {
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), FBIOPAN_DISPLAY, self.pan_vinfo.as_ptr());
            }
        }
    }
}

pub fn set_backlight(level: u32) {
    if let Ok(mut f) = OpenOptions::new().write(true).open(BACKLIGHT_DEV) {
        let _ = writeln!(f, "{}", level);
    }
}

pub fn set_blank(blank: bool) {
    if let Ok(mut f) = OpenOptions::new().write(true).open(BLANK_DEV) {
        let _ = writeln!(f, "{}", if blank { 1 } else { 0 });
    }
}

pub fn set_screen_state(on: bool) {
    if on {
        set_blank(false);
        set_backlight(140);
    } else {
        set_backlight(0);
        set_blank(true);
    }
}
