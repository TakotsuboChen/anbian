mod framebuffer;
mod input;
mod metrics;
mod render;
mod time_sync;

use std::fs;
use std::process::Command;

use framebuffer::{set_screen_state, Framebuffer};
use input::InputWatcher;
use metrics::MetricsCollector;
use render::Renderer;

fn ensure_sshd_alive() {
    let output = Command::new("pgrep").arg("sshd").output();
    let is_running = match output {
        Ok(out) => !out.stdout.is_empty(),
        Err(_) => false,
    };

    if !is_running {
        let _ = fs::create_dir_all("/run/sshd");
        let _ = Command::new("/usr/sbin/sshd").spawn();
        println!("SSHD 守护检查: 已自动拉起 /usr/sbin/sshd");
    }
}

fn main() {
    println!("=== Anbian 硬件仪表盘服务启动 ===");

    // 1. 保活 SSHD
    ensure_sshd_alive();

    // 2. 自动设置上海时区并启动网络自动校时
    time_sync::init_time_sync();

    // 3. 初始化 Framebuffer 硬件抽象
    let mut fb = match Framebuffer::new() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("错误: 无法打开 Framebuffer: {}", e);
            return;
        }
    };

    // 4. 初始化输入事件监听器 (双击检测状态机)
    let mut watcher = InputWatcher::new();

    // 5. 初始化指标收集器与渲染引擎
    let mut collector = MetricsCollector::new();
    let renderer = Renderer::new();

    // 6. 初始状态: 双击常亮模式 (默认开机点亮)
    let mut is_awake = true;
    set_screen_state(true);

    // 渲染并呈现首帧画面
    let initial_metrics = collector.collect();
    let initial_buf = renderer.render(&initial_metrics);
    fb.write_triple_buffer(&initial_buf);
    println!("首帧画面渲染完成，进入主事件调度循环...");

    loop {
        if is_awake {
            // 常亮模式: 每秒刷新一次并监听输入 (超时 1000ms)
            let metrics = collector.collect();
            let buf = renderer.render(&metrics);
            fb.write_triple_buffer(&buf);

            let toggled = watcher.poll_for_toggle(1000);
            if toggled {
                println!("检测到双击或关屏操作: 关闭屏幕并进入深度休眠省电...");
                set_screen_state(false);
                is_awake = false;
            }
        } else {
            // 休眠模式: poll 无限阻塞等待输入，CPU 占用 0.00%
            let toggled = watcher.poll_for_toggle(-1);
            if toggled {
                println!("检测到双击或唤醒操作: 点亮屏幕进入常亮模式...");
                ensure_sshd_alive();
                is_awake = true;
                set_screen_state(true);

                // 立即渲染唤醒后的最新数据
                let metrics = collector.collect();
                let buf = renderer.render(&metrics);
                fb.write_triple_buffer(&buf);
            }
        }
    }
}
