#import <InputMethodKit/InputMethodKit.h>
#import <objc/runtime.h>

static void LucidNote(NSString *line) {
    NSString *path = [NSHomeDirectory() stringByAppendingPathComponent:@"Library/Logs/Lucid/objc-keys.log"];
    NSString *text = [line stringByAppendingString:@"\n"];
    NSFileHandle *handle = [NSFileHandle fileHandleForWritingAtPath:path];
    if (handle == nil) {
        [[NSFileManager defaultManager] createFileAtPath:path contents:[text dataUsingEncoding:NSUTF8StringEncoding] attributes:nil];
        return;
    }
    [handle seekToEndOfFile];
    [handle writeData:[text dataUsingEncoding:NSUTF8StringEncoding]];
    [handle closeFile];
}

static BOOL LucidAccept(id self, NSString *chars, NSInteger keyCode, NSUInteger flags, id client, NSString *source) {
    LucidNote([NSString stringWithFormat:@"%@ key=%ld chars=%lu client=%@", source, (long)keyCode, (unsigned long)chars.length, client]);
    if (chars.length == 0 || client == nil) {
        return NO;
    }
    SEL input = @selector(lucidInputText:key:modifiers:client:);
    if (![self respondsToSelector:input]) {
        LucidNote(@"missing lucidInputText");
        return NO;
    }
    BOOL (*impl)(id, SEL, id, NSInteger, NSUInteger, id) =
        (BOOL (*)(id, SEL, id, NSInteger, NSUInteger, id))[self methodForSelector:input];
    return impl(self, input, chars, keyCode, flags, client);
}

static BOOL LucidInputTextKey(id self, SEL _cmd, NSString *chars, NSInteger keyCode, NSUInteger flags, id client) {
    (void)_cmd;
    return LucidAccept(self, chars ?: @"", keyCode, flags, client, @"inputText:key");
}

static BOOL LucidInputText(id self, SEL _cmd, NSString *chars, id client) {
    (void)_cmd;
    return LucidAccept(self, chars ?: @"", -1, 0, client, @"inputText");
}

static BOOL LucidHandleEvent(id self, SEL _cmd, NSEvent *event, id client) {
    (void)_cmd;
    if (event == nil || event.type != NSEventTypeKeyDown) {
        return NO;
    }
    // Cocoa text fields usually use inputText:, while editor/WebView hosts may
    // deliver only handleEvent:. Both entry points are installed below.
    NSString *chars = event.characters ?: @"";
    // A key event may contain more than one UTF-16 code unit (for example an
    // emoji or a composed character). Let the Rust side decide whether the
    // complete string is printable instead of dropping it here.
    if (chars.length > 0) {
        return LucidAccept(self, chars, (NSInteger)event.keyCode, event.modifierFlags, client, @"handleEvent");
    }
    return NO;
}

void LucidInstallHandleEvent(void) {
    Class cls = NSClassFromString(@"LucidInputController");
    if (cls == Nil) {
        LucidNote(@"class missing");
        return;
    }
    class_replaceMethod(cls, @selector(inputText:key:modifiers:client:), (IMP)LucidInputTextKey, "c@:@qQ@");
    class_replaceMethod(cls, @selector(inputText:client:), (IMP)LucidInputText, "c@:@@");
    // Some hosts (notably editor/webview based apps) deliver only handleEvent:
    // and never call inputText:. Install both entry points. The Rust controller
    // deduplicates the rare case where a host sends both for one physical key.
    class_replaceMethod(cls, @selector(handleEvent:client:), (IMP)LucidHandleEvent, "c@:@@");
    LucidNote(@"installed all input entries");
}
