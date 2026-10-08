//! 设计系统的数值与配色，逐条抄自 mac 侧的 crates/app-mac/src/views.rs。
//!
//! 两边共用同一份 CSS 出处，所以间距/字号/圆角必须一字不差；会分叉的只有颜色：
//! mac 的控件是半透明色叠在 NSVisualEffectView 的毛玻璃上，这边窗口是不透明的，
//! 同一个 rgba 叠到实测底色上算出的是实心值。所以这里存的是「合成之后」的 COLORREF，
//! 而不是原始 alpha —— 看图对不上时该怀疑的是叠底，不是 alpha 写错了。

use windows::Win32::Foundation::COLORREF;

/* ---------- 间距：4pt 网格（与 views.rs 同一套 S1..S4） ---------- */
/// 光学微调档（点与文字、圆角与基线之间）
pub const S1: f32 = 4.0;
/// 组内：一行里控件之间
pub const S2: f32 = 8.0;
/// 行间：正文一叠行之间的固定缝隙（CSS: #panel gap 12）
pub const S3: f32 = 12.0;
/// 边距：面板左右与底部
pub const S4: f32 = 16.0;

pub const PAD_X: f32 = S4;
pub const PAD_TOP: f32 = S2;
pub const PAD_BOTTOM: f32 = S4;
pub const GAP: f32 = S2;
pub const ROW_GAP: f32 = S3;

pub const WIDTH: f32 = 400.0;
pub const CHIP_H: f32 = 44.0;
/// .icon-btn 22×22 → 24：可点面积原本小于视觉预期
pub const ICON: f32 = 24.0;
pub const CTRL_H: f32 = 24.0;
/// 一行文字的高度（CSS: .line-slot 16）
pub const LINE_H: f32 = 16.0;
pub const DOT: f32 = 8.0;
/// 会话浏览器一行
pub const BROWSE_ROW: f32 = 36.0;

pub const T_BODY: f32 = 12.0;
pub const T_META: f32 = 11.0;
pub const T_HEAD: f32 = 13.0;

pub const R_CTRL: f32 = 6.0;
pub const R_FIELD: f32 = 8.0;

/* ---------- 颜色 ---------- */

/// 单通道 alpha 合成。const fn 里用不了闭包，只能一个通道一个通道地摆。
const fn blend(chan: u32, top: u8, a: u8) -> u32 {
    (chan * (255 - a as u32) + (top as u32) * a as u32) / 255
}

/// 把 rgba(r,g,b,a) 合成到 base 上。COLORREF 的字节序是 0x00BBGGRR，**低位字节是红**；
/// 传进来的 r/g/b 是常规顺序。换错顺序会得到一眼看得出但说不出为什么的怪颜色，
/// 所以 tests 里有一条专测这个的。
pub const fn mix(base: COLORREF, r: u8, g: u8, b: u8, a: u8) -> COLORREF {
    let x = base.0;
    COLORREF(
        blend(x & 0xFF, r, a)
            | blend((x >> 8) & 0xFF, g, a) << 8
            | blend((x >> 16) & 0xFF, b, a) << 16,
    )
}

/// 无材质窗口上 style.css `--bg rgba(22,22,24,.4)` 的实测合成值 (9,9,10)（M0 量的）
pub const CARD: COLORREF = COLORREF(0x000A_0909);
/// 面板比 chip 多叠一层 veil(20,20,24,.55)，同样压在黑底上 → (11,11,13)
pub const PANEL: COLORREF = COLORREF(0x000D_0B0B);

/// --line 发丝：只用于容器与分组，控件一律不描边
pub const HAIRLINE: COLORREF = mix(PANEL, 255, 255, 255, 26);
/// fill_ctrl：中性控件的填充（CSS 的 .07 在暗底上几乎不可见，mac 提到 .10）
pub const FILL: COLORREF = mix(PANEL, 255, 255, 255, 26);
/// fill_track：白 .07，▸ 的底座、模式开关的槽
pub const TRACK: COLORREF = mix(PANEL, 255, 255, 255, 18);
/// fill_field：输入区，黑 .26
pub const FIELD: COLORREF = mix(PANEL, 0, 0, 0, 66);
/// --accent #0A84FF
pub const ACCENT: COLORREF = COLORREF(0x00FF_840A);
pub const ACCENT_HOVER: COLORREF = COLORREF(0x00FF_9419);
/// accent_tint：列表选中行
pub const ACCENT_SOFT: COLORREF = mix(PANEL, 10, 132, 255, 56);
/// --warn #e5a13c
pub const WARN: COLORREF = COLORREF(0x003C_A1E5);
/// secondaryLabel rgba(235,235,245,.6)：状态行与次要文字。
/// chip 叠在 CARD 上是 (144,144,151)、面板叠在 PANEL 上是 (145,145,152)，差 1 不到，
/// 只留一个值，免得两个窗口为了这一点差别各写一套。
pub const DIM: COLORREF = mix(PANEL, 235, 235, 245, 153);
/// labelColor：正文
pub const INK: COLORREF = mix(PANEL, 255, 255, 255, 217);
/// tertiaryLabel：列表副行、时间戳
pub const FAINT: COLORREF = mix(PANEL, 235, 235, 245, 77);
pub const WHITE: COLORREF = COLORREF(0x00FF_FFFF);

#[cfg(test)]
mod tests {
    use super::*;

    /// COLORREF 是 0x00BBGGRR：低位字节是红
    fn rgb(c: COLORREF) -> (u8, u8, u8) {
        (
            (c.0 & 0xFF) as u8,
            ((c.0 >> 8) & 0xFF) as u8,
            ((c.0 >> 16) & 0xFF) as u8,
        )
    }

    /// 只有「红蓝不对称」的输入才能暴露字节序写反：纯红叠纯蓝各一半
    #[test]
    fn mixes_top_down_onto_the_base_channel_by_channel() {
        let (r, g, b) = rgb(mix(COLORREF(0x00FF_0000), 255, 0, 0, 128));
        assert!(r > 100 && b > 100, "红色没落到红通道上：{r},{g},{b}");
        assert_eq!(g, 0);
        assert_eq!(rgb(mix(PANEL, 0, 0, 0, 255)), (0, 0, 0));
        assert_eq!(rgb(mix(PANEL, 255, 255, 255, 0)), rgb(PANEL));
        // secondaryLabel rgba(235,235,245,.6) 压在 CARD 上：红绿被整数除法截到 144，蓝正好 151
        assert_eq!(rgb(mix(CARD, 235, 235, 245, 153)), (144, 144, 151));
    }

    /// 间距/字号/圆角与 mac 侧 views.rs 必须逐条相同，抄漏一条就是两版观感分叉
    #[test]
    fn spacing_matches_the_mac_side_scale() {
        assert_eq!((S2, S4), (8.0, 16.0));
        assert_eq!(PAD_X, S4);
        assert_eq!(GAP, S2);
        assert_eq!(WIDTH - 2.0 * PAD_X, 368.0);
    }
}
