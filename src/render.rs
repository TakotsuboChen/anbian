use chrono::Local;
use fontdue::{Font, FontSettings};
use std::fs;
use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::framebuffer::{LINE_STRIDE, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::metrics::SystemMetrics;

pub struct Theme {
    pub bg: Color,
    pub card_bg: Color,
    pub card_border: Color,
    pub ring_bg: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    #[allow(dead_code)]
    pub text_muted: Color,
    pub green: Color,
    pub blue: Color,
    pub orange: Color,
    pub purple: Color,
    pub red: Color,
}

impl Theme {
    pub fn new() -> Self {
        Self {
            bg: Color::from_rgba8(11, 15, 25, 255),
            card_bg: Color::from_rgba8(21, 27, 40, 255),
            card_border: Color::from_rgba8(36, 45, 64, 255),
            ring_bg: Color::from_rgba8(30, 38, 56, 255),
            text_primary: Color::from_rgba8(240, 246, 252, 255),
            text_secondary: Color::from_rgba8(139, 148, 158, 255),
            text_muted: Color::from_rgba8(90, 99, 110, 255),
            green: Color::from_rgba8(46, 160, 67, 255),
            blue: Color::from_rgba8(88, 166, 255, 255),
            orange: Color::from_rgba8(210, 153, 34, 255),
            purple: Color::from_rgba8(188, 140, 255, 255),
            red: Color::from_rgba8(248, 81, 73, 255),
        }
    }
}

pub struct Renderer {
    pub theme: Theme,
    font_cn: Option<Font>,
    font_en: Option<Font>,
}

impl Renderer {
    pub fn new() -> Self {
        let font_cn_data = fs::read("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc").ok();
        let font_en_data =
            fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf").ok();

        let font_cn = font_cn_data.and_then(|d| Font::from_bytes(d, FontSettings::default()).ok());
        let font_en = font_en_data.and_then(|d| Font::from_bytes(d, FontSettings::default()).ok());

        Self {
            theme: Theme::new(),
            font_cn,
            font_en,
        }
    }

    /// 混合文本渲染 (自动回退中英文)
    pub fn draw_text(
        &self,
        pixmap: &mut Pixmap,
        text: &str,
        mut start_x: f32,
        base_y: f32,
        px_size: f32,
        color: Color,
    ) -> f32 {
        for ch in text.chars() {
            // 选择可用字体
            let font = if let Some(ref fe) = self.font_en {
                if fe.lookup_glyph_index(ch) != 0 {
                    fe
                } else if let Some(ref fc) = self.font_cn {
                    fc
                } else {
                    fe
                }
            } else if let Some(ref fc) = self.font_cn {
                fc
            } else {
                continue;
            };

            let (metrics, bitmap) = font.rasterize(ch, px_size);
            let gx = (start_x + metrics.xmin as f32).round() as i32;
            let gy = (base_y - metrics.height as f32 - metrics.ymin as f32).round() as i32;

            if !bitmap.is_empty() && metrics.width > 0 && metrics.height > 0 {
                let r = (color.red() * 255.0) as u32;
                let g = (color.green() * 255.0) as u32;
                let b = (color.blue() * 255.0) as u32;
                let base_a = color.alpha();

                let pw = pixmap.width() as i32;
                let ph = pixmap.height() as i32;

                for row in 0..metrics.height {
                    let py = gy + row as i32;
                    if py < 0 || py >= ph {
                        continue;
                    }
                    for col in 0..metrics.width {
                        let px = gx + col as i32;
                        if px < 0 || px >= pw {
                            continue;
                        }

                        let alpha_val = bitmap[row * metrics.width + col];
                        if alpha_val == 0 {
                            continue;
                        }

                        let norm_a = (alpha_val as f32 / 255.0) * base_a;
                        let inv_a = 1.0 - norm_a;

                        let pixel_idx = (py as usize * pw as usize + px as usize) * 4;
                        let pixels = pixmap.data_mut();

                        let orig_r = pixels[pixel_idx] as f32;
                        let orig_g = pixels[pixel_idx + 1] as f32;
                        let orig_b = pixels[pixel_idx + 2] as f32;

                        pixels[pixel_idx] = ((r as f32 * norm_a) + (orig_r * inv_a)).round() as u8;
                        pixels[pixel_idx + 1] =
                            ((g as f32 * norm_a) + (orig_g * inv_a)).round() as u8;
                        pixels[pixel_idx + 2] =
                            ((b as f32 * norm_a) + (orig_b * inv_a)).round() as u8;
                        pixels[pixel_idx + 3] = 255;
                    }
                }
            }

            start_x += metrics.advance_width;
        }

        start_x
    }

    /// 居中绘制文本
    pub fn draw_text_centered(
        &self,
        pixmap: &mut Pixmap,
        text: &str,
        center_x: f32,
        base_y: f32,
        px_size: f32,
        color: Color,
    ) {
        let width = self.measure_text(text, px_size);
        self.draw_text(
            pixmap,
            text,
            center_x - (width / 2.0),
            base_y,
            px_size,
            color,
        );
    }

    pub fn measure_text(&self, text: &str, px_size: f32) -> f32 {
        let mut total_w = 0.0;
        for ch in text.chars() {
            let font = if let Some(ref fe) = self.font_en {
                if fe.lookup_glyph_index(ch) != 0 {
                    fe
                } else if let Some(ref fc) = self.font_cn {
                    fc
                } else {
                    fe
                }
            } else if let Some(ref fc) = self.font_cn {
                fc
            } else {
                continue;
            };

            let metrics = font.metrics(ch, px_size);
            total_w += metrics.advance_width;
        }
        total_w
    }

    /// 绘制抗锯齿圆环仪表盘 (Circular Ring Gauge)
    pub fn draw_ring_gauge(
        &self,
        pixmap: &mut Pixmap,
        cx: f32,
        cy: f32,
        radius: f32,
        stroke_width: f32,
        percent: f32,
        fg_color: Color,
    ) {
        let stroke = Stroke {
            width: stroke_width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };

        // 1. 背景底环 (360度灰色完整暗环)
        let mut pb_bg = PathBuilder::new();
        let bg_steps = 72;
        for i in 0..=bg_steps {
            let angle = (i as f32 / bg_steps as f32) * std::f32::consts::TAU;
            let x = cx + radius * angle.cos();
            let y = cy + radius * angle.sin();
            if i == 0 {
                pb_bg.move_to(x, y);
            } else {
                pb_bg.line_to(x, y);
            }
        }
        if let Some(path_bg) = pb_bg.finish() {
            let mut paint_bg = Paint::default();
            paint_bg.set_color(self.theme.ring_bg);
            paint_bg.anti_alias = true;
            pixmap.stroke_path(&path_bg, &paint_bg, &stroke, Transform::identity(), None);
        }

        // 2. 前景进度弧 (从 12 点钟方向 -PI/2 顺时针延伸)
        let p_clamped = percent.clamp(0.0, 100.0);
        if p_clamped > 0.5 {
            let mut pb_fg = PathBuilder::new();
            let start_angle = -std::f32::consts::FRAC_PI_2;
            let sweep_angle = (p_clamped / 100.0) * std::f32::consts::TAU;
            let fg_steps = ((p_clamped / 100.0) * 72.0).max(4.0) as usize;

            for i in 0..=fg_steps {
                let a = start_angle + (sweep_angle * (i as f32 / fg_steps as f32));
                let x = cx + radius * a.cos();
                let y = cy + radius * a.sin();
                if i == 0 {
                    pb_fg.move_to(x, y);
                } else {
                    pb_fg.line_to(x, y);
                }
            }

            if let Some(path_fg) = pb_fg.finish() {
                let mut paint_fg = Paint::default();
                paint_fg.set_color(fg_color);
                paint_fg.anti_alias = true;
                pixmap.stroke_path(&path_fg, &paint_fg, &stroke, Transform::identity(), None);
            }
        }
    }

    /// 绘制通用圆角卡片方块
    pub fn draw_card_box(
        &self,
        pixmap: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        title: &str,
        tag_color: Color,
    ) {
        let mut pb = PathBuilder::new();
        let r = 24.0;
        // 绘制圆角矩形
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.quad_to(x, y + h, x, y + h - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);

        if let Some(path) = pb.finish() {
            // 背景填充
            let mut paint_fill = Paint::default();
            paint_fill.set_color(self.theme.card_bg);
            paint_fill.anti_alias = true;
            pixmap.fill_path(
                &path,
                &paint_fill,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );

            // 边框描边
            let mut paint_border = Paint::default();
            paint_border.set_color(self.theme.card_border);
            paint_border.anti_alias = true;
            let stroke = Stroke {
                width: 2.0,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint_border, &stroke, Transform::identity(), None);
        }

        // 卡片左上角小色块标识 (增大尺寸)
        let mut paint_tag = Paint::default();
        paint_tag.set_color(tag_color);
        let tag_rect = Rect::from_xywh(x + 24.0, y + 24.0, 7.0, 32.0).unwrap();
        pixmap.fill_rect(tag_rect, &paint_tag, Transform::identity(), None);

        // 卡片标题 (增大字号至 30pt)
        self.draw_text(
            pixmap,
            title,
            x + 40.0,
            y + 49.0,
            30.0,
            self.theme.text_primary,
        );
    }

    /// 全屏渲染函数
    pub fn render(&self, metrics: &SystemMetrics) -> Vec<u8> {
        let mut content = Pixmap::new(SCREEN_WIDTH, SCREEN_HEIGHT).unwrap();
        content.fill(self.theme.bg);

        let margin = 44.0;
        let col_gap = 24.0;
        let card_w = (SCREEN_WIDTH as f32 - (margin * 2.0) - col_gap) / 2.0; // 484.0

        // ================= 1. 顶部 Header (大字号、醒目、无废话) =================
        let now = Local::now();
        let clock_str = now.format("%H:%M:%S").to_string();
        let date_str = now.format("%Y年%m月%d日").to_string();
        let weekday_str = match now.format("%u").to_string().as_str() {
            "1" => "星期一",
            "2" => "星期二",
            "3" => "星期三",
            "4" => "星期四",
            "5" => "星期五",
            "6" => "星期六",
            _ => "星期日",
        };

        // 超大时钟数字 (字号提升至 102.0 pt)
        self.draw_text(
            &mut content,
            &clock_str,
            margin,
            125.0,
            102.0,
            self.theme.text_primary,
        );

        // 日期与运行时间 (字号提升至 30.0 pt)
        let sub_info = format!("{} {}   ·   系统运行 {}", date_str, weekday_str, metrics.uptime_str);
        self.draw_text(
            &mut content,
            &sub_info,
            margin,
            180.0,
            30.0,
            self.theme.text_secondary,
        );

        // 右上角指示：高质感抗锯齿实心发光圆点 + 36pt 超大字号“运行中”
        let tag_x = SCREEN_WIDTH as f32 - margin - 180.0;
        let dot_cx = tag_x + 12.0;
        let dot_cy = 78.0;

        // 1. 发光外光晕圈
        let mut pb_glow = PathBuilder::new();
        let glow_r = 15.0;
        let steps = 36;
        for i in 0..=steps {
            let a = (i as f32 / steps as f32) * std::f32::consts::TAU;
            let px = dot_cx + glow_r * a.cos();
            let py = dot_cy + glow_r * a.sin();
            if i == 0 { pb_glow.move_to(px, py); } else { pb_glow.line_to(px, py); }
        }
        if let Some(path_glow) = pb_glow.finish() {
            let mut paint_glow = Paint::default();
            paint_glow.set_color(Color::from_rgba8(46, 160, 67, 85));
            paint_glow.anti_alias = true;
            content.fill_path(&path_glow, &paint_glow, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        // 2. 核心发光实心圆
        let mut pb_dot = PathBuilder::new();
        let core_r = 9.0;
        for i in 0..=steps {
            let a = (i as f32 / steps as f32) * std::f32::consts::TAU;
            let px = dot_cx + core_r * a.cos();
            let py = dot_cy + core_r * a.sin();
            if i == 0 { pb_dot.move_to(px, py); } else { pb_dot.line_to(px, py); }
        }
        if let Some(path_dot) = pb_dot.finish() {
            let mut paint_dot = Paint::default();
            paint_dot.set_color(self.theme.green);
            paint_dot.anti_alias = true;
            content.fill_path(&path_dot, &paint_dot, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }

        // 3. 超大字号“运行中” (36pt)
        self.draw_text(
            &mut content,
            "运行中",
            tag_x + 32.0,
            90.0,
            36.0,
            self.theme.green,
        );

        // 分割线
        let mut line_paint = Paint::default();
        line_paint.set_color(self.theme.card_border);
        let div_rect = Rect::from_xywh(margin, 220.0, SCREEN_WIDTH as f32 - (margin * 2.0), 2.0).unwrap();
        content.fill_rect(div_rect, &line_paint, Transform::identity(), None);

        // ================= 2. 两列方块式主网格 (两行方块，扩大高度与圆环) =================
        let card_h = 490.0;
        let row1_y = 250.0;
        let ring_radius = 102.0;
        let ring_stroke = 18.0;

        // ----- 卡片 1 (左上): 处理器 -----
        let col1_x = margin;
        self.draw_card_box(&mut content, col1_x, row1_y, card_w, card_h, "处理器", self.theme.orange);
        let ring1_cx = col1_x + (card_w / 2.0);
        let ring1_cy = row1_y + 195.0;
        let cpu_color = if metrics.cpu_percent > 85.0 { self.theme.red } else { self.theme.orange };
        self.draw_ring_gauge(&mut content, ring1_cx, ring1_cy, ring_radius, ring_stroke, metrics.cpu_percent, cpu_color);
        // 圆心数字 (56pt，在半径 102 的超大圆环中空间充裕，绝不贴边)
        let cpu_str = format!("{:.0}%", metrics.cpu_percent);
        self.draw_text_centered(&mut content, &cpu_str, ring1_cx, ring1_cy + 20.0, 56.0, self.theme.text_primary);
        // 下方详细指标
        self.draw_text_centered(&mut content, &format!("核心温度: {:.1} ℃", metrics.cpu_temp), ring1_cx, row1_y + 380.0, 28.0, self.theme.orange);
        self.draw_text_centered(&mut content, &format!("当前主频: {}", metrics.cpu_load), ring1_cx, row1_y + 430.0, 26.0, self.theme.text_secondary);

        // ----- 卡片 2 (右上): 运行内存 -----
        let col2_x = margin + card_w + col_gap;
        self.draw_card_box(&mut content, col2_x, row1_y, card_w, card_h, "运行内存", self.theme.purple);
        let ring2_cx = col2_x + (card_w / 2.0);
        let ring2_cy = row1_y + 195.0;
        self.draw_ring_gauge(&mut content, ring2_cx, ring2_cy, ring_radius, ring_stroke, metrics.mem_percent, self.theme.purple);
        // 圆心数字
        let mem_str = format!("{:.0}%", metrics.mem_percent);
        self.draw_text_centered(&mut content, &mem_str, ring2_cx, ring2_cy + 20.0, 56.0, self.theme.text_primary);
        // 下方详细指标
        self.draw_text_centered(&mut content, &format!("已用: {} MB / {} MB", metrics.mem_used_mb, metrics.mem_total_mb), ring2_cx, row1_y + 380.0, 28.0, self.theme.text_primary);
        self.draw_text_centered(&mut content, &format!("Swap: {} MB ({:.0}%)", metrics.swap_used_mb, metrics.swap_percent), ring2_cx, row1_y + 430.0, 26.0, self.theme.text_secondary);

        // ----- 第二行方块 (y: row2_y) -----
        let row2_y = row1_y + card_h + 30.0; // 250 + 490 + 30 = 770.0

        // ----- 卡片 3 (左下): 电池与功耗 -----
        self.draw_card_box(&mut content, col1_x, row2_y, card_w, card_h, "电池与功耗", self.theme.green);
        let ring3_cx = col1_x + (card_w / 2.0);
        let ring3_cy = row2_y + 195.0;
        let bat_color = if metrics.battery_cap <= 20 { self.theme.red } else { self.theme.green };
        self.draw_ring_gauge(&mut content, ring3_cx, ring3_cy, ring_radius, ring_stroke, metrics.battery_cap as f32, bat_color);
        // 圆心数字
        let bat_str = format!("{}%", metrics.battery_cap);
        self.draw_text_centered(&mut content, &bat_str, ring3_cx, ring3_cy + 20.0, 56.0, self.theme.text_primary);
        // 下方指标 (只显示瓦时 Wh 与实时功率 W)
        self.draw_text_centered(&mut content, &format!("剩余电量: {:.1} Wh", metrics.battery_wh), ring3_cx, row2_y + 380.0, 28.0, self.theme.green);
        let pwr_str = format!("{} · 功率 {:.2} W", metrics.battery_status, metrics.battery_power_w);
        self.draw_text_centered(&mut content, &pwr_str, ring3_cx, row2_y + 430.0, 26.0, self.theme.text_secondary);

        // ----- 卡片 4 (右下): 存储空间 -----
        self.draw_card_box(&mut content, col2_x, row2_y, card_w, card_h, "存储空间", self.theme.blue);
        let ring4_cx = col2_x + (card_w / 2.0);
        let ring4_cy = row2_y + 195.0;
        self.draw_ring_gauge(&mut content, ring4_cx, ring4_cy, ring_radius, ring_stroke, metrics.disk_percent, self.theme.blue);
        // 圆心数字
        let disk_str = format!("{:.0}%", metrics.disk_percent);
        self.draw_text_centered(&mut content, &disk_str, ring4_cx, ring4_cy + 20.0, 56.0, self.theme.text_primary);
        // 下方指标
        self.draw_text_centered(&mut content, &format!("已用: {:.1} GB / {:.1} GB", metrics.disk_used_gb, metrics.disk_total_gb), ring4_cx, row2_y + 380.0, 28.0, self.theme.text_primary);
        self.draw_text_centered(&mut content, &format!("使用率: {:.1}%", metrics.disk_percent), ring4_cx, row2_y + 430.0, 26.0, self.theme.text_secondary);

        // ================= 3. 网络连接卡片 (去除双语英文与括号) =================
        let net_y = row2_y + card_h + 30.0; // 770 + 490 + 30 = 1290.0
        let net_w = SCREEN_WIDTH as f32 - (margin * 2.0);
        let net_h = 550.0;
        self.draw_card_box(&mut content, margin, net_y, net_w, net_h, "网络连接", self.theme.green);

        let net_left = margin + 44.0;
        let line_gap = 105.0;

        // Line 1: WiFi 名称
        self.draw_text(&mut content, "WiFi 名称:", net_left, net_y + 120.0, 30.0, self.theme.text_secondary);
        self.draw_text(&mut content, &metrics.wifi_ssid, net_left + 220.0, net_y + 120.0, 34.0, self.theme.green);

        // Line 2: 局域网 IP
        self.draw_text(&mut content, "局域网 IP:", net_left, net_y + 120.0 + line_gap, 30.0, self.theme.text_secondary);
        self.draw_text(&mut content, &metrics.lan_ip, net_left + 220.0, net_y + 120.0 + line_gap, 34.0, self.theme.blue);

        // Line 3: 公网 IP
        self.draw_text(&mut content, "公网 IP:", net_left, net_y + 120.0 + (line_gap * 2.0), 30.0, self.theme.text_secondary);
        self.draw_text(&mut content, &metrics.wan_ip, net_left + 220.0, net_y + 120.0 + (line_gap * 2.0), 34.0, self.theme.orange);

        // Line 4: 累计流量
        self.draw_text(&mut content, "累计流量:", net_left, net_y + 120.0 + (line_gap * 3.0), 30.0, self.theme.text_secondary);
        let traffic_str = format!("↓ 接收 {:.1} MB     ↑ 发送 {:.1} MB", metrics.rx_mb, metrics.tx_mb);
        self.draw_text(&mut content, &traffic_str, net_left + 220.0, net_y + 120.0 + (line_gap * 3.0), 32.0, self.theme.text_primary);

        // ================= 4. 彻底删除双击文案方块，底部纯净留白 =================

        // ================= 5. 旋转 180 度并适配硬件步幅 =================
        let mut rotated = Pixmap::new(SCREEN_WIDTH, SCREEN_HEIGHT).unwrap();
        let src_data = content.data();
        let dst_data = rotated.data_mut();
        let total_pixels = (SCREEN_WIDTH * SCREEN_HEIGHT) as usize;

        for i in 0..total_pixels {
            let inv_i = total_pixels - 1 - i;
            let src_idx = i * 4;
            let dst_idx = inv_i * 4;
            dst_data[dst_idx] = src_data[src_idx];
            dst_data[dst_idx + 1] = src_data[src_idx + 1];
            dst_data[dst_idx + 2] = src_data[src_idx + 2];
            dst_data[dst_idx + 3] = src_data[src_idx + 3];
        }

        let mut final_buf = vec![0u8; (LINE_STRIDE * SCREEN_HEIGHT) as usize];
        let rot_bytes = rotated.data();
        let src_row_bytes = (SCREEN_WIDTH * 4) as usize;
        let dst_row_bytes = LINE_STRIDE as usize;

        for row in 0..SCREEN_HEIGHT as usize {
            let src_offset = row * src_row_bytes;
            let dst_offset = row * dst_row_bytes;
            final_buf[dst_offset..dst_offset + src_row_bytes]
                .copy_from_slice(&rot_bytes[src_offset..src_offset + src_row_bytes]);
        }

        final_buf
    }
}
