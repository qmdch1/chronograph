/// 32x32 RGBA 시계 아이콘을 픽셀 배열로 반환합니다.
pub fn icon_rgba() -> (Vec<u8>, u32, u32) {
    const W: u32 = 32;
    const H: u32 = 32;
    let mut pixels = vec![0u8; (W * H * 4) as usize];

    let cx = 15.5f32;
    let cy = 15.5f32;

    for y in 0..H {
        for x in 0..W {
            let idx = ((y * W + x) * 4) as usize;
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            // 바깥 원 테두리 (r=14..15.5)
            let on_ring = dist >= 13.2 && dist <= 15.5;
            // 안쪽 배경 (r<13.2)
            let inside = dist < 13.2;

            // 시침 (12시 방향에서 -30도 = 330도, 길이 6)
            let hour_angle: f32 = -std::f32::consts::PI / 2.0 - std::f32::consts::PI / 6.0;
            let on_hour = on_hand(dx, dy, hour_angle, 5.5, 1.3);

            // 분침 (12시 방향에서 90도, 길이 9)
            let min_angle: f32 = -std::f32::consts::PI / 2.0 + std::f32::consts::PI / 2.0;
            let on_min = on_hand(dx, dy, min_angle, 8.5, 1.0);

            // 중심 점
            let on_center = dist < 1.8;

            let (r, g, b, a) = if on_ring {
                (220, 220, 220, 255)
            } else if inside && (on_hour || on_min || on_center) {
                (255, 255, 255, 255)
            } else if inside {
                (45, 45, 55, 230)
            } else {
                (0, 0, 0, 0)
            };

            pixels[idx]     = r;
            pixels[idx + 1] = g;
            pixels[idx + 2] = b;
            pixels[idx + 3] = a;
        }
    }

    (pixels, W, H)
}

/// 픽셀 (dx, dy)가 주어진 각도/길이/두께의 침 위에 있는지 판별
fn on_hand(dx: f32, dy: f32, angle: f32, length: f32, half_width: f32) -> bool {
    // 침 방향 단위 벡터
    let ux = angle.cos();
    let uy = angle.sin();
    // 침 방향 투영 (0~length)
    let along = dx * ux + dy * uy;
    // 침 수직 투영
    let perp = (-dx * uy + dy * ux).abs();
    along >= -0.5 && along <= length && perp <= half_width
}

/// tray_icon::Icon 생성
pub fn tray_icon() -> anyhow::Result<tray_icon::Icon> {
    let (rgba, w, h) = icon_rgba();
    Ok(tray_icon::Icon::from_rgba(rgba, w, h)?)
}

/// egui 윈도우 아이콘 생성
pub fn viewport_icon() -> egui::viewport::IconData {
    let (rgba, w, h) = icon_rgba();
    egui::viewport::IconData { rgba, width: w, height: h }
}
