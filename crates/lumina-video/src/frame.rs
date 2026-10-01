//! 帧源抽象:v0.1 不接 ffmpeg/symphonia(依赖契约),解码走 trait。
//!
//! v0.2 真解码器按分册六 §1.3 走进程隔离,届时实现本 trait 即可无缝接入
//! 预览监视器(docs/04 §12:解码线程 → 容量 2 通道 → UI 取最新帧)。
//! [`SyntheticSource`] 彩条合成源让无解码环境的示例/测试有帧可放。

use serde::{Deserialize, Serialize};

/// 一帧 RGBA8 像素(直通 alpha)。`data.len() == width * height * 4`,行优先。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// 帧对应的时间轴毫秒(播放头对齐用)。
    pub ms: u64,
    pub width: u32,
    pub height: u32,
    /// RGBA8,行优先,每像素 4 字节。
    pub data: Vec<u8>,
}

impl Frame {
    /// 读取 (x, y) 处像素 RGBA;越界返回 `None`。
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ])
    }
}

/// 帧源:时间轴毫秒 → 帧。实现方负责解码/合成;`frame_at` 返回 `None`
/// 表示该时刻无帧(素材尽头/解码失败),预览层据此落回最近缓存帧。
pub trait FrameSource {
    /// 固有尺寸(像素)。
    fn size(&self) -> (u32, u32);

    fn frame_at(&mut self, ms: u64) -> Option<Frame>;
}

/// 彩条合成源:7 竖条 R/G/B/C/M/Y/白 + 底部随 ms 变化的灰度带。
///
/// 纯确定性:相同 (尺寸, ms) 恒产出相同字节,示例/测试可精确断言;
/// 无限长(`frame_at` 恒返回 `Some`)。
pub struct SyntheticSource {
    width: u32,
    height: u32,
}

impl SyntheticSource {
    /// 尺寸钳制为至少 1×1。
    pub fn new(width: u32, height: u32) -> Self {
        SyntheticSource {
            width: width.max(1),
            height: height.max(1),
        }
    }

    /// 7 标准彩条(R/G/B/C/M/Y/白)。
    pub const BAR_COLORS: [[u8; 3]; 7] = [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [0, 255, 255],
        [255, 0, 255],
        [255, 255, 0],
        [255, 255, 255],
    ];

    /// 底部灰度带取值:每 40ms 步进 1,循环 0..256。
    pub fn band_value(ms: u64) -> u8 {
        ((ms / 40) % 256) as u8
    }

    fn band_height(&self) -> u32 {
        (self.height / 8).max(1)
    }
}

impl FrameSource for SyntheticSource {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn frame_at(&mut self, ms: u64) -> Option<Frame> {
        let (w, h) = (self.width, self.height);
        let band_start = h.saturating_sub(self.band_height());
        let mut data = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            for x in 0..w {
                // 彩条按列均分 7 份;x < w 保证列号恒 < 7
                let bar = Self::BAR_COLORS[x as usize * 7 / w as usize];
                let rgb = if y >= band_start {
                    let g = Self::band_value(ms);
                    [g, g, g]
                } else {
                    bar
                };
                data.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        Some(Frame {
            ms,
            width: w,
            height: h,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_and_buffer_layout() {
        let mut src = SyntheticSource::new(64, 32);
        assert_eq!(src.size(), (64, 32));
        let frame = src.frame_at(0).expect("合成源恒有帧");
        assert_eq!(frame.width, 64);
        assert_eq!(frame.height, 32);
        assert_eq!(frame.ms, 0);
        assert_eq!(frame.data.len(), 64 * 32 * 4, "RGBA8 行优先满缓冲");
        // (63,31) 在底部灰度带内,ms=0 → 灰度 0,alpha 直通 255
        assert_eq!(frame.pixel(63, 31).expect("界内"), [0, 0, 0, 255]);
    }

    #[test]
    fn pixel_accessor_bounds() {
        let mut src = SyntheticSource::new(8, 8);
        let frame = src.frame_at(0).expect("frame");
        assert!(frame.pixel(8, 0).is_none(), "x 越界");
        assert!(frame.pixel(0, 8).is_none(), "y 越界");
        assert!(frame.pixel(7, 7).is_some());
    }

    #[test]
    fn min_size_clamped_to_one() {
        let mut src = SyntheticSource::new(0, 0);
        assert_eq!(src.size(), (1, 1));
        assert!(src.frame_at(0).is_some());
    }

    #[test]
    fn color_bar_column_mapping_is_stable() {
        // 70px 宽 → 每条 10px;在彩条区(顶部)逐条取中点采样
        let mut src = SyntheticSource::new(70, 16);
        let frame = src.frame_at(1234).expect("frame");
        for (bar, expected) in SyntheticSource::BAR_COLORS.iter().enumerate() {
            let x = bar as u32 * 10 + 5;
            let px = frame.pixel(x, 0).expect("界内");
            assert_eq!(px[0..3], *expected, "第 {bar} 条中点颜色");
            assert_eq!(px[3], 255, "直通 alpha");
        }
        // 相同 ms 两次产出逐字节一致(确定性)
        let again = src.frame_at(1234).expect("frame");
        assert_eq!(again.data, frame.data);
    }

    #[test]
    fn grayscale_band_varies_with_ms() {
        let mut src = SyntheticSource::new(32, 16);
        let band_y = 15; // 高度 16 → 带高 2,底部两行是灰度带
        let f0 = src.frame_at(0).expect("frame");
        let f200 = src.frame_at(200).expect("frame");
        let p0 = f0.pixel(0, band_y).expect("界内");
        let p200 = f200.pixel(0, band_y).expect("界内");
        assert_eq!(p0[0], SyntheticSource::band_value(0));
        assert_eq!(p200[0], SyntheticSource::band_value(200));
        assert_ne!(p0[0], p200[0], "灰度带随 ms 变化");
        // 顶部彩条区不随 ms 变化
        assert_eq!(f0.pixel(0, 0), f200.pixel(0, 0));
        // 帧携带其毫秒
        assert_eq!(f200.ms, 200);
    }

    #[test]
    fn band_value_cycles_every_40ms() {
        assert_eq!(SyntheticSource::band_value(0), 0);
        assert_eq!(SyntheticSource::band_value(39), 0);
        assert_eq!(SyntheticSource::band_value(40), 1);
        assert_eq!(SyntheticSource::band_value(40 * 256), 0, "256 步循环回 0");
    }
}
