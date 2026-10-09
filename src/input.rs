use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::time::Instant;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct InputEvent {
    pub time: libc::timeval,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

pub struct InputWatcher {
    #[allow(dead_code)]
    files: Vec<std::fs::File>,
    poll_fds: Vec<libc::pollfd>,
    last_tap_time: Option<Instant>,
}

impl InputWatcher {
    pub fn new() -> Self {
        let devs = [
            "/dev/input/event7", // mtk-tpd 触摸屏
            "/dev/input/event4", // gf-keys 指纹/Home 键
            "/dev/input/event3", // mtk-kpd 电源/音量键
        ];

        let mut files = Vec::new();
        let mut poll_fds = Vec::new();

        for dev in &devs {
            if let Ok(file) = OpenOptions::new().read(true).open(dev) {
                let fd = file.as_raw_fd();
                unsafe {
                    let flags = libc::fcntl(fd, libc::F_GETFL, 0);
                    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
                poll_fds.push(libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                });
                files.push(file);
                println!("已监听输入设备: {}", dev);
            }
        }

        Self {
            files,
            poll_fds,
            last_tap_time: None,
        }
    }

    /// 等待并处理输入事件。
    /// timeout_ms: > 0 时等待指定毫秒 (常亮刷新模式)；-1 时无限阻塞 (休眠省电模式)。
    /// 返回值: 如果检测到了双击或电源键开关切换，返回 `true`。
    pub fn poll_for_toggle(&mut self, timeout_ms: i32) -> bool {
        if self.poll_fds.is_empty() {
            if timeout_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
            }
            return false;
        }

        let ret = unsafe {
            libc::poll(
                self.poll_fds.as_mut_ptr(),
                self.poll_fds.len() as libc::nfds_t,
                timeout_ms,
            )
        };

        if ret <= 0 {
            return false;
        }

        let mut toggle_triggered = false;
        let event_size = std::mem::size_of::<InputEvent>();

        for i in 0..self.poll_fds.len() {
            if (self.poll_fds[i].revents & libc::POLLIN) != 0 {
                let fd = self.poll_fds[i].fd;
                let mut buf = [0u8; 1024];

                loop {
                    let n = unsafe {
                        libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
                    };
                    if n <= 0 {
                        break;
                    }

                    let count = n as usize / event_size;
                    for k in 0..count {
                        let ev: InputEvent = unsafe {
                            std::ptr::read_unaligned(
                                buf.as_ptr().add(k * event_size) as *const InputEvent
                            )
                        };

                        // 1. 电源键 (KEY_POWER = 116)，按下即触发切换
                        if ev.type_ == 1 && ev.code == 116 && ev.value == 1 {
                            toggle_triggered = true;
                        }

                        // 2. 触摸屏点击 (BTN_TOUCH = 330) 或按键松开
                        // 判定双击 (Double Tap)
                        if (ev.type_ == 1 && ev.code == 330 && ev.value == 0)
                            || (ev.type_ == 1 && ev.code == 158 && ev.value == 0) // Back键
                            || (ev.type_ == 1 && ev.code == 102 && ev.value == 0)
                        // Home键
                        {
                            let now = Instant::now();
                            if let Some(last) = self.last_tap_time {
                                let elapsed = now.duration_since(last).as_millis();
                                if elapsed >= 50 && elapsed <= 450 {
                                    // 命中双击！
                                    toggle_triggered = true;
                                    self.last_tap_time = None;
                                    continue;
                                }
                            }
                            self.last_tap_time = Some(now);
                        }
                    }
                }
            }
        }

        toggle_triggered
    }
}
