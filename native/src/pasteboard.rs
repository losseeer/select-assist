//! 剪贴板读写、时间戳、站点打开。
//! 对应 Electron 的 clipboard.* / new Date().toLocaleTimeString() / shell.openExternal。

use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSPasteboardWriting, NSWorkspace};
use objc2_foundation::{NSArray, NSDate, NSDateFormatter, NSString, NSURL};

pub fn change_count() -> isize {
    NSPasteboard::generalPasteboard().changeCount()
}

pub fn read_text() -> Option<String> {
    NSPasteboard::generalPasteboard()
        .stringForType(unsafe { NSPasteboardTypeString })
        .map(|text| text.to_string())
}

/// 写纯文本；调用方负责把 lastClip 同步成它，否则自己的写入会点亮未读红点
pub fn write_text(text: &str) -> bool {
    let board = NSPasteboard::generalPasteboard();
    let string = NSString::from_str(text);
    let writing: &ProtocolObject<dyn NSPasteboardWriting> = ProtocolObject::from_ref(&*string);
    board.clearContents();
    board.writeObjects(&NSArray::arrayWithObject(writing))
}

/// tooltip 里的时间：HH:mm:ss，与 Electron 在 zh-CN 下 toLocaleTimeString() 的显示一致
pub fn local_time() -> String {
    let formatter = NSDateFormatter::new();
    formatter.setDateFormat(Some(&NSString::from_str("HH:mm:ss")));
    formatter.stringFromDate(&NSDate::now()).to_string()
}

/// 只放 http/https（照搬 index.ts 的 site:open），其余返回 false 而不是丢给系统
pub fn open_site(url: &str) -> bool {
    let Some(target) = NSURL::URLWithString(&NSString::from_str(url)) else {
        return false;
    };
    let scheme = target.scheme().map(|s| s.to_string()).unwrap_or_default();
    if scheme != "https" && scheme != "http" {
        return false;
    }
    NSWorkspace::sharedWorkspace().openURL(&target)
}
