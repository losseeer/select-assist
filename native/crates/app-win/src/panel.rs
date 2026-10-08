//! panel 的视图：纯绘制 + 命中，没有窗口、没有状态。
//!
//! 与 paint.rs（chip 视图）同构：给它一份 PanelView，它画出像素并回吐一张
//! 「名字 -> 矩形」的命中表；窗口、消息与动作都在 shell.rs。这么分是因为绘制和命中
//! 必须在同一帧用同一份布局算，分成两处算就会差半个像素地对不上。
//!
//! 版面照 static/index.html 的 #panel 与 mac 侧 panel.rs 的行序：头部固定，正文是
//! 一叠「按模式与折叠状态出现或消失的行」，高度由可见行累加，所以从不写死。

use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_RIGHT, HDC, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use crate::draw::{self, RectF};
use crate::theme::{
    ACCENT, ACCENT_HOVER, ACCENT_SOFT, BROWSE_ROW, CARD, CTRL_H, DIM, FAINT, FIELD, FILL, GAP,
    HAIRLINE, ICON, INK, LINE_H, PAD_BOTTOM, PAD_TOP, PAD_X, ROW_GAP, R_CTRL, R_FIELD, S1, S2,
    TRACK, T_BODY, T_HEAD, T_META, WARN, WHITE, WIDTH,
};

/// 可点的东西。shell.rs 拿这个名字去分派动作，所以这里每加一个 id 就必须在那边加一个
/// 分支 —— 编译器不会提醒，别靠记忆。
pub type Id = &'static str;

const LEFT: DRAW_TEXT_FORMAT = DT_LEFT;
const CENTER: DRAW_TEXT_FORMAT = DT_CENTER;
const RIGHT: DRAW_TEXT_FORMAT = DT_RIGHT;

/// 会话浏览器的一行。文字全部由 pack::row_text 算好，视图只负责摆
#[derive(Clone, Default)]
pub struct BrowserRow {
    /// "agent #短id"
    pub head: String,
    pub title: String,
    /// "3 时"
    pub time: String,
    pub preview: String,
}

#[derive(Default)]
pub struct PanelView {
    pub status: String,
    pub status_err: bool,
    /// true = 会话解读，false = 选区直通
    pub read_mode: bool,
    pub agent: String,
    pub turns: usize,
    pub session_line: String,
    pub session_err: bool,
    /// 上下文那一行右侧的即时状态（"正在填充上下文…" / 失败原因）
    pub ctx_status: String,
    pub ctx_err: bool,
    pub browsing: bool,
    pub browser_rows: Vec<BrowserRow>,
    pub browser_sel: Option<usize>,
    pub prompt_name: String,
    pub sites: Vec<String>,
    pub pack_meta: String,
    /// 复制刚成功：文案换成「已复制 ✓」，由 shell 定时收回
    pub copied: bool,
    pub hover: Option<Id>,
    /// 悬停在哪一行会话上（hover 只带一个名字，行号得另说）
    pub hover_row: Option<usize>,
}

/// 一帧的绘制上下文：DC、DPI 换算、以及边走边收集的命中表。
/// 这些原先是四个函数参数，加上控件自己的几个就撞上 clippy 的参数上限了。
/// 一块命中区。idx 是同一个 id 下的序号 —— 会话浏览器的每一行、站点按钮的每一个，
/// id 都一样，只有序号能分派「点的是哪一个」
pub struct Hit {
    pub id: Id,
    pub r: RectF,
    pub idx: Option<usize>,
}

pub struct Pen {
    hdc: HDC,
    hwnd: HWND,
    s: f32,
    hits: Vec<Hit>,
}

impl Pen {
    fn round(&self, r: RectF, radius: f32, color: COLORREF) {
        draw::fill_round(self.hdc, r.to_native(self.s), radius, self.s, color);
    }

    fn fill(&self, r: RectF, color: COLORREF) {
        draw::fill(self.hdc, r.to_native(self.s), color);
    }

    /// 一行文字。字号每档一张字体，所以 Pen 里带着 hwnd 去算 DPI
    fn line(&self, text: &str, r: RectF, color: COLORREF, pt: f32, align: DRAW_TEXT_FORMAT) {
        draw::select_font(self.hdc, self.hwnd, pt);
        draw::text(
            self.hdc,
            text,
            r.to_native(self.s),
            color,
            align | DT_END_ELLIPSIS | draw::line(),
        );
    }

    /// 控件宽度：文字实测 + CSS 的 padding 4px 12px
    fn pill_width(&self, text: &str) -> f32 {
        draw::measure(self.hdc, text, T_BODY, self.s) + 24.0
    }

    /// 主色胶囊（取入选区 / 复制）。`from_right` 时 x 给的是右边界；返回宽度，
    /// 好让调用方接着往左排下一个
    fn pill(&mut self, id: Id, label: &str, x: f32, y: f32, from_right: bool, hot: bool) -> f32 {
        let w = self.pill_width(label);
        let r = RectF::new(if from_right { x - w } else { x }, y, w, CTRL_H);
        self.round(r, R_CTRL, if hot { ACCENT_HOVER } else { ACCENT });
        self.line(label, r, WHITE, T_BODY, CENTER);
        self.hits.push(Hit { id, r, idx: None });
        w
    }

    /// 中性胶囊（浏览 / 刷新 / 三个下拉）
    fn button(&mut self, id: Id, label: &str, x: f32, y: f32, w: f32, hot: bool) {
        let r = RectF::new(x, y, w, CTRL_H);
        self.round(r, R_CTRL, if hot { FILL } else { TRACK });
        self.line(label, r, INK, T_BODY, CENTER);
        self.hits.push(Hit { id, r, idx: None });
    }

    /// 图标位（▾ / ✕）：只有字形，socket 决定要不要给一块底
    fn icon(&mut self, id: Id, glyph: &str, x: f32, y: f32, socket: bool) {
        let r = RectF::new(x, y, ICON, CTRL_H);
        if socket {
            self.round(r, R_CTRL, TRACK);
        }
        self.line(glyph, r, DIM, T_BODY, CENTER);
        self.hits.push(Hit { id, r, idx: None });
    }
}

/* ---------- 各行 ---------- */

fn head(p: &mut Pen, v: &PanelView, y: f32) -> f32 {
    p.icon("collapse", "▾", PAD_X, y, true);
    let quit_x = WIDTH - PAD_X - ICON;
    p.icon("quit", "✕", quit_x, y, false);
    let capture_w = p.pill_width("取入选区");
    p.pill(
        "capture",
        "取入选区",
        quit_x - GAP - capture_w,
        y,
        false,
        v.hover == Some("capture"),
    );
    let status_x = PAD_X + ICON + GAP;
    p.line(
        &v.status,
        RectF::new(
            status_x,
            y,
            quit_x - GAP - capture_w - GAP - status_x,
            CTRL_H,
        ),
        if v.status_err { WARN } else { DIM },
        T_BODY,
        LEFT,
    );
    y + CTRL_H
}

/// 模式开关 + agent / 轮数。直通模式下右侧换成一句说明 —— 那两个下拉那时没有作用
fn mode_row(p: &mut Pen, v: &PanelView, y: f32) -> f32 {
    const SEG: f32 = 148.0;
    const INSET: f32 = 2.0;
    let track = RectF::new(PAD_X, y, SEG, CTRL_H);
    p.round(track, R_CTRL, TRACK);
    let half = (SEG - 3.0 * INSET) / 2.0;
    for (i, (name, on)) in [("会话解读", v.read_mode), ("选区直通", !v.read_mode)]
        .iter()
        .enumerate()
    {
        let r = RectF::new(
            PAD_X + INSET + i as f32 * (half + INSET),
            y + INSET,
            half,
            CTRL_H - 2.0 * INSET,
        );
        if *on {
            p.round(r, R_CTRL, FILL);
        }
        p.line(name, r, if *on { INK } else { DIM }, T_BODY, CENTER);
        // 点当前已选中的那一段什么也不做，所以分成两个 id，shell 那边少一次判断
        p.hits.push(Hit {
            id: if *on { "mode-on" } else { "mode-off" },
            r,
            idx: None,
        });
    }

    if v.read_mode {
        let turns_w = 64.0;
        let agent_w = 112.0;
        p.button(
            "turns",
            &format!("{} 轮 ▾", v.turns),
            WIDTH - PAD_X - turns_w,
            y,
            turns_w,
            v.hover == Some("turns"),
        );
        p.button(
            "agent",
            &format!("{} ▾", v.agent),
            WIDTH - PAD_X - turns_w - GAP - agent_w,
            y,
            agent_w,
            v.hover == Some("agent"),
        );
    } else {
        p.line(
            "逐字节原文，翻译 / 检索即用",
            RectF::new(
                track.right + GAP,
                y,
                WIDTH - PAD_X - track.right - GAP,
                CTRL_H,
            ),
            FAINT,
            T_BODY,
            RIGHT,
        );
    }
    y + CTRL_H
}

fn ctx_row(p: &mut Pen, v: &PanelView, y: f32) -> f32 {
    let browse_w = p.pill_width("浏览会话");
    let refresh_w = p.pill_width("刷新");
    p.button(
        "browse",
        "浏览会话",
        PAD_X,
        y,
        browse_w,
        v.hover == Some("browse"),
    );
    let x = PAD_X + browse_w + GAP;
    p.button(
        "refresh",
        "刷新",
        x,
        y,
        refresh_w,
        v.hover == Some("refresh"),
    );
    let text_x = x + refresh_w + GAP;
    p.line(
        &v.ctx_status,
        RectF::new(text_x, y, WIDTH - PAD_X - text_x, CTRL_H),
        if v.ctx_err { WARN } else { DIM },
        T_BODY,
        RIGHT,
    );
    y + CTRL_H
}

/// 会话浏览器：最多五行。一行三块（agent #id · 标题 · 距今）加一行摘要，
/// 列宽与 mac 侧的 BROWSER_LEFT / BROWSER_TIME 一致
fn browser(p: &mut Pen, v: &PanelView, y: f32) -> f32 {
    const HEAD_W: f32 = 118.0;
    const TIME_W: f32 = 62.0;
    let shown = v.browser_rows.len().clamp(1, 5);
    let field = RectF::new(PAD_X, y, WIDTH - 2.0 * PAD_X, BROWSE_ROW * shown as f32);
    p.round(field, R_FIELD, FIELD);
    for (i, row) in v.browser_rows.iter().take(5).enumerate() {
        let top = y + i as f32 * BROWSE_ROW;
        let r = RectF::new(PAD_X, top, field.width(), BROWSE_ROW);
        if v.browser_sel == Some(i) {
            p.round(r, R_FIELD, ACCENT_SOFT);
        } else if v.hover_row == Some(i) {
            p.round(r, R_FIELD, HAIRLINE);
        }
        let x = r.left + S2;
        let w = r.width() - 2.0 * S2;
        p.line(
            &row.head,
            RectF::new(x, top + 3.0, HEAD_W, LINE_H),
            DIM,
            T_META,
            LEFT,
        );
        p.line(
            &row.title,
            RectF::new(x + HEAD_W + S1, top + 2.0, w - HEAD_W - S1 - TIME_W, LINE_H),
            INK,
            T_BODY,
            LEFT,
        );
        p.line(
            &row.time,
            RectF::new(x + w - TIME_W, top + 3.0, TIME_W, LINE_H),
            FAINT,
            T_META,
            RIGHT,
        );
        p.line(
            &row.preview,
            RectF::new(x, top + 20.0, w, LINE_H - 2.0),
            DIM,
            T_META,
            LEFT,
        );
        p.hits.push(Hit {
            id: "browser",
            r,
            idx: Some(i),
        });
    }
    if v.browser_rows.is_empty() {
        p.line(
            "没有可浏览的会话",
            RectF::new(field.left + S2, y, field.width() - 2.0 * S2, BROWSE_ROW),
            FAINT,
            T_BODY,
            LEFT,
        );
    }
    y + field.height()
}

/// 输出组：指令下拉 + 复制 + 站点按钮
fn output(p: &mut Pen, v: &PanelView, y: f32) -> f32 {
    let copy_label = if v.copied { "已复制 ✓" } else { "复制" };
    let prompt_w = 150.0;
    p.button(
        "prompt",
        &format!("{} ▾", v.prompt_name),
        PAD_X,
        y,
        prompt_w,
        v.hover == Some("prompt"),
    );
    let copy_x = PAD_X + prompt_w + GAP;
    let copy_w = p.pill(
        "copy",
        copy_label,
        copy_x,
        y,
        false,
        v.hover == Some("copy"),
    );
    // 站点从右边界倒着排；放不下就不画（面板不横向滚动，挤成一团更难看）
    let limit = copy_x + copy_w + GAP;
    // 从右往左量、从左往右画：idx 必须跟 v.sites 同序，否则点第三个会开出第四个
    let mut slots = Vec::new();
    let mut x = WIDTH - PAD_X;
    for (i, name) in v.sites.iter().enumerate().rev() {
        let w = p.pill_width(name);
        if x - w < limit {
            break;
        }
        x -= w;
        slots.push((i, name.clone(), x, w));
        x -= GAP;
    }
    for (i, name, sx, w) in slots.into_iter().rev() {
        p.button("site", &name, sx, y, w, v.hover == Some("site"));
        if let Some(last) = p.hits.last_mut() {
            last.idx = Some(i);
        }
    }
    y + CTRL_H
}

/* ---------- 入口 ---------- */

/// 一帧画完的结果：命中区表 + 内容应有的高度（DIP）
#[derive(Default)]
pub struct Layout {
    hits: Vec<Hit>,
    pub height: f32,
}

impl Layout {
    /// 倒着查：后画的叠在上面，先命中上面那个
    pub fn hit(&self, x: f32, y: f32) -> Option<(Id, Option<usize>)> {
        self.hits
            .iter()
            .rev()
            .find(|h| h.r.contains(x, y))
            .map(|h| (h.id, h.idx))
    }
}

/// 画一帧，返回命中表与内容应有的高度
pub fn paint(hwnd: HWND, v: &PanelView) -> Layout {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let (pw, ph) = (rc.right.max(1), rc.bottom.max(1));
    let s = draw::scale(hwnd);
    let mut p = Pen {
        hdc: HDC::default(),
        hwnd,
        s,
        hits: Vec::new(),
    };

    unsafe {
        let sdc = GetDC(Some(hwnd));
        let mem = CreateCompatibleDC(Some(sdc));
        let bmp = CreateCompatibleBitmap(sdc, pw, ph);
        let old = SelectObject(mem, bmp.into());
        p.hdc = mem;
        draw::select_font(mem, hwnd, T_BODY);
        draw::fill(mem, rc, CARD);

        let mut y = PAD_TOP;
        y = head(&mut p, v, y) + GAP;
        p.line(
            &v.session_line,
            RectF::new(PAD_X, y, WIDTH - 2.0 * PAD_X, LINE_H),
            if v.session_err { WARN } else { INK },
            T_HEAD,
            LEFT,
        );
        y += LINE_H + GAP;
        y = mode_row(&mut p, v, y) + ROW_GAP;
        if v.read_mode {
            y = ctx_row(&mut p, v, y) + ROW_GAP;
        }
        if v.browsing {
            y = browser(&mut p, v, y) + ROW_GAP;
        }
        p.fill(
            RectF::new(PAD_X, y - ROW_GAP / 2.0, WIDTH - 2.0 * PAD_X, 1.0),
            HAIRLINE,
        );
        y = output(&mut p, v, y);
        if !v.pack_meta.is_empty() {
            y += GAP;
            p.line(
                &v.pack_meta,
                RectF::new(PAD_X, y, WIDTH - 2.0 * PAD_X, LINE_H - 2.0),
                FAINT,
                T_META,
                LEFT,
            );
            y += LINE_H;
        }
        let height = y + PAD_BOTTOM;

        let _ = BitBlt(sdc, 0, 0, pw, ph, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(Some(hwnd), sdc);
        Layout {
            hits: p.hits,
            height,
        }
    }
}
