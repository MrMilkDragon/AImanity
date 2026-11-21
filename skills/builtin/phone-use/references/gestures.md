# 手势和运动

用 phone_observe(ocr_mode="sequence") 取得按时间排列的两帧。motion_tracks 的 bounds 是纹理块在后一帧的位置，dx 和 dy 以截图像素表示位移，向右和向下为正。结合轮廓、背景参照和滚动方向辨认目标。

需要提前出手时，在 phone_tap 或 phone_swipe 的 when 中指定目标将进入的 region、motion_direction 和 timeout_ms（100–15000）。可用 track_from 锁定目标纹理，lead_ms 预留动作时间。背景也在移动时，用实屏采样的 colors 或 sample_points 及 coverage_percent 区分目标；any_color 接受替代特征色，avoid_points 排除干扰，clear_region 检查空闲区。静态变色可设 on_appearance。目标已出现或看不清时重新观察。动作后读回画面。

phone_scroll 在原生容器内滚动，distance 为容器跨度的 0.1–0.8，duration_ms 为 100–1500。定位条目先短滚 0.25–0.4；没有移动时检查可滚动区域、方向和加载状态。长按一般从 550–750 ms 开始，拖动用 phone_drag。

phone_gesture 接收 1–5 指同步 paths。每指 pointer_id 唯一，从 at_ms=0 到 duration_ms（100–3000）；使用最近完整截图的坐标。捏合时两指围绕可见中心相向或相离，旋转时画多段弧线，平移时保持间距。pressure 为 0.1–1 的归一化触压信号。用动作后的缩放、位置或角度变化验证结果。

phone_rotate(rotation=0/1/2/3) 改变屏幕方向，旋转后重新定位。phone_shake 的 amplitude 单位为 m/s²，frequency_hz 为振动频率；可从 3–5 m/s²、2–3 Hz、600–900 ms 开始，观察应用是否响应。
