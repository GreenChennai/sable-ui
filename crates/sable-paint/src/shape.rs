//! 形状生成原语(迭代计划 08 E10)。
//!
//! [`squircle`] 生成 iOS 同款连续曲率圆角(超椭圆角):四角各用
//! `|x/a|^n + |y/a|^n = 1` 的超椭圆象限采样,直线连接相邻角。`n = 2`
//! 精确退化为普通圆角(圆),`n ≈ 4~5` 即 iOS 卡片常用的 squircle。

use kurbo::{BezPath, Point, Rect};

/// 每个角的超椭圆采样段数(24 段/象限:弦高 ≈ 0.0005×半径,远小于 0.5px)。
const CORNER_SEGMENTS: usize = 24;

/// 平滑度下限:`n < 2` 的超椭圆比圆更"尖",低于圆角无设计意义,钳到 2。
const MIN_SMOOTHNESS: f64 = 2.0;

/// 生成连续曲率圆角矩形(squircle,E10)。
///
/// - `rect`:目标矩形(四角超椭圆内切于此);
/// - `radius`:圆角半径,自动钳制到 `[0, min(w,h)/2]`(过大/负值不 panic);
/// - `smoothness`:超椭圆指数 `n`(`|x/a|^n + |y/a|^n = 1`),`n = 2` 退化
///   为普通圆角,推荐 3~5;`NaN`/小于 2 一律按 2 处理。
///
/// 路径 = 四段超椭圆折线 + 四条直线(无直线空间时直线段长度为 0,无害),
/// 已闭合,可直接交给 [`crate::sink::PaintSink`] 绘制。
pub fn squircle(rect: Rect, radius: f64, smoothness: f64) -> BezPath {
    // 钳制:f64::max/min 对 NaN 返回另一侧,故 NaN 半径→0、NaN 平滑度→2
    let radius = radius.clamp(0.0, rect.width().min(rect.height()) / 2.0);
    let radius = if radius.is_nan() { 0.0 } else { radius };
    let n = smoothness.max(MIN_SMOOTHNESS);

    let (x0, y0, x1, y1) = (rect.x0, rect.y0, rect.x1, rect.y1);
    let r = radius;
    // 四角圆心与超椭圆象限的外向符号:(sx, sy) 决定角弧从水平极端点
    // (C.x + sx·r, C.y) 扫到竖直极端点 (C.x, C.y + sy·r)。
    let corners = [
        // (圆心, sx, sy) —— 左上、右上、右下、左下
        (Point::new(x0 + r, y0 + r), -1.0, -1.0),
        (Point::new(x1 - r, y0 + r), 1.0, -1.0),
        (Point::new(x1 - r, y1 - r), 1.0, 1.0),
        (Point::new(x0 + r, y1 - r), -1.0, 1.0),
    ];

    // 顺时针遍历:TL 弧(t:0→1,从左边点扫到顶边点)→ 顶边 → TR 弧
    // (反向:t:1→0)→ 右边 → BR 弧(正向)→ 底边 → BL 弧(反向)→ 左边。
    // 弧采样**含两端极端点**(t=0/1),四个角的水平/竖直极端点都是显式
    // 顶点,bbox 精确等于 rect 且点集对水平/垂直镜像严格对称;
    // 接缝处的重复顶点(零长度线段)对填充渲染无害。
    let mut path = BezPath::new();
    let tl = superellipse_arc(corners[0], r, n, false);
    if let Some(first) = tl.first().copied() {
        path.move_to(first);
    }
    for point in tl.iter().skip(1) {
        path.line_to(*point);
    }
    path.line_to(Point::new(x1 - r, y0)); // 顶边
    for point in superellipse_arc(corners[1], r, n, true) {
        path.line_to(point);
    }
    path.line_to(Point::new(x1, y1 - r)); // 右边
    for point in superellipse_arc(corners[2], r, n, false) {
        path.line_to(point);
    }
    path.line_to(Point::new(x0 + r, y1)); // 底边
    for point in superellipse_arc(corners[3], r, n, true) {
        path.line_to(point);
    }
    path.line_to(Point::new(x0, y0 + r)); // 左边回起点
    path.close_path();
    path
}

/// 单角超椭圆象限采样:点 = C + (sx·r·X(t), sy·r·Y(t)),
/// `X(t) = cos(πt/2)^(2/n)`,`Y(t) = sin(πt/2)^(2/n)`,t ∈ [0, 1]。
///
/// `rev = true` 输出 t:1→0(反向)。
///
/// n=2 时 X=cos、Y=sin,采样点精确落在半径 r 的圆弧上(kurbo 的圆角
/// 是同一圆弧的三次贝塞尔近似,两者偏差即测试的对照基准)。
fn superellipse_arc(corner: (Point, f64, f64), r: f64, n: f64, rev: bool) -> Vec<Point> {
    let (center, sx, sy) = corner;
    let exp = 2.0 / n;
    let mut points = Vec::with_capacity(CORNER_SEGMENTS + 1);
    for i in 0..=CORNER_SEGMENTS {
        // 端点直接取解析极值:cos(f64::PI/2) ≈ 6.1e-17 而非 0,
        // 乘上大半径会让极端点漂出矩形边(对称性/(bbox)测试都靠端点精确)
        let (xu, yu) = if i == 0 {
            (1.0, 0.0)
        } else if i == CORNER_SEGMENTS {
            (0.0, 1.0)
        } else {
            let theta = core::f64::consts::FRAC_PI_2 * f64::from(i as u32)
                / f64::from(CORNER_SEGMENTS as u32);
            (theta.cos().powf(exp), theta.sin().powf(exp))
        };
        let dx = sx * r * xu;
        let dy = sy * r * yu;
        points.push(Point::new(center.x + dx, center.y + dy));
    }
    if rev {
        points.reverse();
    }
    points
}

#[cfg(test)]
mod tests {
    use kurbo::Shape;

    use super::*;

    /// 收集路径全部顶点(本模块只产 line_to,顶点即采样点)。
    fn vertices(path: &BezPath) -> Vec<Point> {
        path.elements()
            .iter()
            .copied()
            .filter_map(|el| match el {
                kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn bounding_box_equals_input_rect() {
        let rect = Rect::new(10.0, 20.0, 110.0, 80.0);
        for n in [2.0, 3.0, 5.0] {
            let path = squircle(rect, 24.0, n);
            let bbox = path.bounding_box();
            assert!(
                (bbox.x0 - rect.x0).abs() < 1e-9
                    && (bbox.y0 - rect.y0).abs() < 1e-9
                    && (bbox.x1 - rect.x1).abs() < 1e-9
                    && (bbox.y1 - rect.y1).abs() < 1e-9,
                "n={n}:采样极值必须触达矩形四边,实际 {bbox:?}"
            );
        }
    }

    /// n=2 与 kurbo 圆角(同一圆弧的三次贝塞尔近似)逐点对照 < 0.5px:
    /// 对本实现的每个采样点,用 kurbo 的 `ParamCurveNearest::nearest` 求到
    /// kurbo::RoundedRect 路径(直线+三次贝塞尔)的**精确最近距离**
    /// (kurbo 圆弧近似自身误差 ~0.03% 半径,远小于 0.5px 阈值)。
    #[test]
    fn smoothness_two_matches_kurbo_rounded_rect() {
        use kurbo::{ParamCurveNearest, Shape};

        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        let radius = 20.0;
        let mine = squircle(rect, radius, 2.0);
        let kurbo_path = kurbo::RoundedRect::new(0.0, 0.0, 100.0, 60.0, radius).to_path(0.1);
        let segments: Vec<kurbo::PathSeg> = kurbo_path.segments().collect();
        assert!(segments.len() >= 8, "kurbo 参考路径应含 4 直线 + 4 圆角");

        for p in vertices(&mine) {
            let nearest = segments
                .iter()
                .map(|seg| seg.nearest(p, 1e-6).distance_sq.sqrt())
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest < 0.5,
                "n=2 采样点 {p:?} 距 kurbo 圆角路径最近 {nearest}px ≥ 0.5px"
            );
        }
    }

    #[test]
    fn shape_is_symmetric_across_both_axes() {
        let rect = Rect::new(-30.0, -10.0, 70.0, 50.0);
        let path = squircle(rect, 16.0, 4.0);
        let points = vertices(&path);
        let cx = (rect.x0 + rect.x1) / 2.0;
        let cy = (rect.y0 + rect.y1) / 2.0;
        // 弧采样含两端极端点后,镜像点都是显式顶点,对称是精确的
        const TOL: f64 = 1e-9;
        for p in &points {
            let mirrored = Point::new(2.0 * cx - p.x, p.y);
            assert!(
                points
                    .iter()
                    .any(|q| (q.x - mirrored.x).abs() < TOL && (q.y - mirrored.y).abs() < TOL),
                "点 {p:?} 的水平镜像 {mirrored:?} 不在路径里"
            );
            let mirrored_v = Point::new(p.x, 2.0 * cy - p.y);
            assert!(
                points
                    .iter()
                    .any(|q| (q.x - mirrored_v.x).abs() < TOL && (q.y - mirrored_v.y).abs() < TOL),
                "点 {p:?} 的垂直镜像 {mirrored_v:?} 不在路径里"
            );
        }
    }

    #[test]
    fn clamps_radius_and_smoothness_safely() {
        let rect = Rect::new(0.0, 0.0, 40.0, 20.0);
        // 半径超过半短边 → 钳到 10;路径仍有效且 bbox 正确
        let path = squircle(rect, 999.0, 2.0);
        let bbox = path.bounding_box();
        assert!((bbox.x1 - 40.0).abs() < 1e-9 && (bbox.y1 - 20.0).abs() < 1e-9);
        // NaN/负输入:不 panic,退化为安全形状
        let _ = squircle(rect, f64::NAN, f64::NAN);
        let _ = squircle(rect, -5.0, 1.0);
        // 半径 0 → 纯矩形:四角即矩形顶点
        let sharp = squircle(rect, 0.0, 2.0);
        let points = vertices(&sharp);
        for corner in [
            Point::new(0.0, 0.0),
            Point::new(40.0, 0.0),
            Point::new(40.0, 20.0),
            Point::new(0.0, 20.0),
        ] {
            assert!(
                points
                    .iter()
                    .any(|p| (p.x - corner.x).abs() < 1e-9 && (p.y - corner.y).abs() < 1e-9),
                "r=0 时矩形角点 {corner:?} 必须在路径上"
            );
        }
    }
}
