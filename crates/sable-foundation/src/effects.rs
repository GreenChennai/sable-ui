//! 节点效果数据模型(分册七 E 系列;迭代计划 08 S4 #4.1)。
//!
//! 本模块只做**纯数据 + 纯函数**:效果条目如何挂在 [`crate::scene::Node`] 上、
//! 各类效果的参数语义与常用预设构造器。像素求值在 sable-paint
//! (`effects::apply_effects_rgba`,E6/E7 的 CPU 路径),撤销走 command.rs 的
//! `AddEffect`/`RemoveEffect`/`MoveEffect`/`SetEffectEnabled`/`SetEffectSpec`
//! 五命令(`project.rs` 的 `SerializedCommand` 已同步镜像)。
//!
//! # 语义(分册七/Illustrator 外观面板)
//!
//! 节点的 `effects` 是**有序**列表:渲染时按序应用到"该节点单独渲染的
//! 结果"上,再把结果合成回画布。`enabled = false` 的条目整条跳过(等价于
//! 临时删除,但保留参数供重新启用)。
//!
//! # E11 效果 × 动画联动(接线说明)
//!
//! 参数拖动(如效果面板的 NumberField)→ 每次增量发一条
//! `SetEffectSpec { old, new }`,`History` 的 merge 范式把同节点同 index 的
//! 连续拖动合并为**一步撤销**(与 SetFill 拖色板同款,A4/interact 现成模式)。
//! 动画侧同理:关键帧插值在帧头构造 `SetEffectSpec`(old = 上一帧值)即可
//! 复用同一条撤销/重做路径,无需新增命令形态。

use serde::{Deserialize, Serialize};

use crate::scene::Rgba8;

/// 节点效果条目(有序挂载,分册七/Illustrator 外观面板语义)。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectEntry {
    /// 效果参数。
    pub spec: EffectSpec,
    /// 启用开关(禁用条目渲染时整条跳过,但保留参数)。
    pub enabled: bool,
}

impl EffectEntry {
    /// 恒等校验便捷方法:条目禁用或效果参数为恒等 → 渲染可整条跳过。
    pub fn is_active(&self) -> bool {
        self.enabled && !self.spec.is_noop()
    }
}

/// 效果种类与参数。
///
/// 求值域约定:**预乘 RGBA8 像素缓冲**(与 vello_cpu/CpuRenderer 输出同
/// 语义);颜色矩阵在直通 alpha 域逐像素求值后重新预乘(见 sable-paint
/// `apply_color_matrix` 的 doc)。半径/偏移单位 = 文档(世界)坐标 px,
/// 渲染侧乘 zoom 换算为像素。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EffectSpec {
    /// 高斯模糊(E1/E12):CPU 路径 = 3×box blur 近似(半径减半,与阴影
    /// 同款约定);GPU = 待真机管线(S3)。
    GaussianBlur {
        /// 模糊半径 px(0 = 无模糊)。
        radius: f64,
    },
    /// 投影(E4,参数语义与 `render_shadow_rgba` 一致,颜色泛化为任意色)。
    DropShadow {
        /// 模糊半径 px(0 = 硬影)。
        blur: f64,
        /// 偏移(世界坐标;+x 向右、+y 向下)。
        offset: [f64; 2],
        /// 阴影颜色(直通 alpha;渲染时预乘)。
        color: Rgba8,
    },
    /// 发光(E7):`inner = true` 内发光(蒙版在形状内侧),否则外发光
    /// (等价于零偏移彩色投影,绘制在形状之下)。
    Glow {
        /// 光晕扩散半径 px。
        radius: f64,
        /// 光晕颜色。
        color: Rgba8,
        /// `true` = 内发光,`false` = 外发光。
        inner: bool,
    },
    /// 颜色矩阵(E6):4×5 矩阵 = 4 行(输出 R,G,B,A)× 4 列(输入
    /// R,G,B,A)+ 第 5 列偏移 [`EffectSpec::offsets`]。
    ///
    /// Brightness/Contrast/Saturation/HueRotate 预设用下方构造器;与 SVG
    /// `feColorMatrix` 的对齐方式见 `apply_color_matrix`(sable-paint)。
    ColorMatrix {
        /// 4×4 系数阵,`matrix[行][列]`,行 = 输出通道、列 = 输入通道。
        matrix: [[f32; 4]; 4],
        /// 第 5 列偏移(加在每个输出通道上)。
        offsets: [f32; 4],
    },
}

impl EffectSpec {
    /// 亮度:RGB 对角阵 ×`scale`(1.0 = 不变;alpha 恒等)。
    pub fn brightness(scale: f32) -> EffectSpec {
        EffectSpec::ColorMatrix {
            matrix: [
                [scale, 0.0, 0.0, 0.0],
                [0.0, scale, 0.0, 0.0],
                [0.0, 0.0, scale, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            offsets: [0.0; 4],
        }
    }

    /// 饱和度:标准饱和矩阵,系数取 **rec.709** 亮度权重
    /// (0.2126/0.7152/0.0722,SVG `feColorMatrix type="saturate"` 规范
    /// 公式;W3C Filter Effects 1.0 原文用 0.213/0.715/0.072 三位小数,
    /// 本实现保留 rec.709 全精度,`s = 0` 时同为灰度化语义)。
    pub fn saturate(s: f32) -> EffectSpec {
        let (lr, lg, lb) = (0.2126f32, 0.7152f32, 0.0722f32);
        EffectSpec::ColorMatrix {
            matrix: [
                [lr + (1.0 - lr) * s, lg * (1.0 - s), lb * (1.0 - s), 0.0],
                [lr * (1.0 - s), lg + (1.0 - lg) * s, lb * (1.0 - s), 0.0],
                [lr * (1.0 - s), lg * (1.0 - s), lb + (1.0 - lb) * s, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            offsets: [0.0; 4],
        }
    }

    /// 色相旋转:标准色相旋转矩阵(SVG `feColorMatrix type="hueRotate"`
    /// 规范公式,绕灰轴的线性近似——对高饱和主色不保亮度/饱和度,这是
    /// 规范本身的性质,不是实现误差)。
    pub fn hue_rotate(degrees: f32) -> EffectSpec {
        let rad = degrees.to_radians();
        let (sin, cos) = rad.sin_cos();
        // 行展开的规范矩阵(第三输入列与前三列对称补齐)
        let m = [
            [
                0.213 + cos * 0.787 - sin * 0.213,
                0.715 - cos * 0.715 - sin * 0.715,
                0.072 - cos * 0.072 + sin * 0.928,
            ],
            [
                0.213 - cos * 0.213 + sin * 0.143,
                0.715 + cos * 0.285 + sin * 0.140,
                0.072 - cos * 0.072 - sin * 0.283,
            ],
            [
                0.213 - cos * 0.213 - sin * 0.787,
                0.715 - cos * 0.715 + sin * 0.715,
                0.072 + cos * 0.928 + sin * 0.072,
            ],
        ];
        EffectSpec::ColorMatrix {
            matrix: [
                [m[0][0], m[0][1], m[0][2], 0.0],
                [m[1][0], m[1][1], m[1][2], 0.0],
                [m[2][0], m[2][1], m[2][2], 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            offsets: [0.0; 4],
        }
    }

    /// 对比度:`c·x + 0.5·(1−c)`(c = 1 不变;c → 0 收敛到中灰)。
    pub fn contrast(c: f32) -> EffectSpec {
        let offset = 0.5 * (1.0 - c);
        EffectSpec::ColorMatrix {
            matrix: [
                [c, 0.0, 0.0, 0.0],
                [0.0, c, 0.0, 0.0],
                [0.0, 0.0, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            offsets: [offset, offset, offset, 0.0],
        }
    }

    /// 效果是否为数学恒等(渲染侧可整条跳过):恒等矩阵/零半径/零偏移/
    /// 全透明色彩判。浮点按**逐位相等**判——预设构造器产出的恒等可精确
    /// 识别;手工构造的"近恒等"矩阵按有效果处理(保守,不猜)。
    pub fn is_noop(&self) -> bool {
        match self {
            EffectSpec::GaussianBlur { radius } => *radius <= 0.0,
            EffectSpec::DropShadow {
                blur,
                offset,
                color,
            } => color[3] == 0 || (*blur <= 0.0 && *offset == [0.0, 0.0]),
            EffectSpec::Glow { radius, color, .. } => *radius <= 0.0 || color[3] == 0,
            EffectSpec::ColorMatrix { matrix, offsets } => {
                *matrix == Self::IDENTITY_MATRIX && *offsets == [0.0f32; 4]
            }
        }
    }

    /// 4×4 恒等阵(`is_noop` 的 ColorMatrix 判据)。
    const IDENTITY_MATRIX: [[f32; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_is_diagonal_scale() {
        let spec = EffectSpec::brightness(2.0);
        let EffectSpec::ColorMatrix { matrix, offsets } = spec else {
            panic!("brightness 应为 ColorMatrix");
        };
        assert_eq!(matrix[0], [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(matrix[1], [0.0, 2.0, 0.0, 0.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 2.0, 0.0]);
        assert_eq!(matrix[3], [0.0, 0.0, 0.0, 1.0], "alpha 行恒等");
        assert_eq!(offsets, [0.0; 4]);
        assert!(!spec.is_noop());
    }

    #[test]
    fn saturate_zero_rows_sum_to_gray_luminance() {
        let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::saturate(0.0) else {
            panic!("saturate 应为 ColorMatrix");
        };
        assert_eq!(offsets, [0.0; 4]);
        // s = 0:每行 = 对应亮度权重(输出 = 灰度),行和 = 1(灰不变灰)
        for row in &matrix[..3] {
            let sum: f32 = row[..3].iter().sum();
            assert!((sum - 1.0).abs() < 1e-6, "去色矩阵行和应为 1,实际 {sum}");
        }
        assert_eq!(matrix[0][0], 0.2126);
        assert_eq!(matrix[0][1], 0.7152);
        assert_eq!(matrix[0][2], 0.0722, "三行应同为 rec.709 权重");
        assert_eq!(matrix[1][0], 0.2126);
        // alpha 行恒等
        assert_eq!(matrix[3], [0.0, 0.0, 0.0, 1.0]);
        assert!(!EffectSpec::saturate(0.0).is_noop());
    }

    #[test]
    fn saturate_one_is_identity() {
        let spec = EffectSpec::saturate(1.0);
        assert!(spec.is_noop(), "saturate(1) 应为恒等矩阵");
    }

    #[test]
    fn hue_rotate_rows_sum_to_one_and_zero_degrees_is_identity() {
        for deg in [0.0f32, 90.0, 180.0, 270.0] {
            let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::hue_rotate(deg) else {
                panic!("hue_rotate 应为 ColorMatrix");
            };
            assert_eq!(offsets, [0.0; 4]);
            for row in &matrix[..3] {
                let sum: f32 = row[..3].iter().sum();
                assert!(
                    (sum - 1.0).abs() < 1e-4,
                    "色相旋转矩阵行和应为 1(灰不变):deg={deg} sum={sum}"
                );
            }
        }
        assert!(EffectSpec::hue_rotate(0.0).is_noop(), "0° 旋转 = 恒等矩阵");
        assert!(!EffectSpec::hue_rotate(180.0).is_noop());
    }

    #[test]
    fn contrast_offsets_follow_formula() {
        let EffectSpec::ColorMatrix { matrix, offsets } = EffectSpec::contrast(0.5) else {
            panic!("contrast 应为 ColorMatrix");
        };
        assert_eq!(matrix[0][0], 0.5);
        assert_eq!(offsets, [0.25, 0.25, 0.25, 0.0], "offset = 0.5·(1−c)");
        assert_eq!(matrix[3], [0.0, 0.0, 0.0, 1.0]);
        assert!(EffectSpec::contrast(1.0).is_noop(), "contrast(1) = 恒等");
    }

    #[test]
    fn is_noop_covers_all_identities() {
        // 模糊:零半径
        assert!(EffectSpec::GaussianBlur { radius: 0.0 }.is_noop());
        assert!(EffectSpec::GaussianBlur { radius: -1.0 }.is_noop());
        assert!(!EffectSpec::GaussianBlur { radius: 2.0 }.is_noop());
        // 投影:零偏移零模糊,或全透明色
        assert!(
            EffectSpec::DropShadow {
                blur: 0.0,
                offset: [0.0, 0.0],
                color: [0, 0, 0, 255],
            }
            .is_noop()
        );
        assert!(
            EffectSpec::DropShadow {
                blur: 8.0,
                offset: [3.0, 0.0],
                color: [0, 0, 0, 0],
            }
            .is_noop(),
            "全透明投影无可见效果"
        );
        assert!(
            !EffectSpec::DropShadow {
                blur: 0.0,
                offset: [2.0, 0.0],
                color: [0, 0, 0, 255],
            }
            .is_noop(),
            "硬影 + 偏移仍可见"
        );
        // 发光
        assert!(
            EffectSpec::Glow {
                radius: 0.0,
                color: [255, 0, 0, 255],
                inner: false,
            }
            .is_noop()
        );
        // 颜色矩阵:恒等阵
        assert!(EffectSpec::brightness(1.0).is_noop());
        // 手工构造的近恒等按有效果处理(保守,逐位判)
        assert!(
            !EffectSpec::ColorMatrix {
                matrix: [
                    [1.000_000_1, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0]
                ],
                offsets: [0.0; 4],
            }
            .is_noop()
        );
    }

    #[test]
    fn entry_is_active_requires_enabled_and_non_noop() {
        let enabled = EffectEntry {
            spec: EffectSpec::GaussianBlur { radius: 2.0 },
            enabled: true,
        };
        assert!(enabled.is_active());
        let disabled = EffectEntry {
            enabled: false,
            ..enabled.clone()
        };
        assert!(!disabled.is_active());
        let noop = EffectEntry {
            spec: EffectSpec::brightness(1.0),
            enabled: true,
        };
        assert!(!noop.is_active());
    }

    #[test]
    fn effect_types_serde_roundtrip() {
        let entries = vec![
            EffectEntry {
                spec: EffectSpec::GaussianBlur { radius: 3.5 },
                enabled: true,
            },
            EffectEntry {
                spec: EffectSpec::DropShadow {
                    blur: 4.0,
                    offset: [2.0, -1.5],
                    color: [0, 0, 0, 128],
                },
                enabled: false,
            },
            EffectEntry {
                spec: EffectSpec::Glow {
                    radius: 6.0,
                    color: [255, 200, 40, 255],
                    inner: true,
                },
                enabled: true,
            },
            EffectEntry {
                spec: EffectSpec::hue_rotate(180.0),
                enabled: true,
            },
            EffectEntry {
                spec: EffectSpec::saturate(0.5),
                enabled: true,
            },
            EffectEntry {
                spec: EffectSpec::contrast(1.2),
                enabled: true,
            },
            EffectEntry {
                spec: EffectSpec::brightness(0.8),
                enabled: true,
            },
        ];
        let json = serde_json::to_string(&entries).expect("序列化");
        let back: Vec<EffectEntry> = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(back, entries, "全部效果变体必须无损往返");
    }
}
